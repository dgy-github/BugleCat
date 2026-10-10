//! Fixed-candidate provenance replay; prepare is offline, live is explicit and bounded.
use async_trait::async_trait;
use ncx_core::verification::types::{
    Claim, ClaimKind, VerificationInput, POLICY_VERSION, TEMPLATE_VERSION,
};
use ncx_core::{
    context_store::ContextStore,
    context_trace::{SourceKind, SourceRecord},
    verification, Session,
};
use ncx_provider::jev::{self, JevTransport};
use rusqlite::{params_from_iter, types::Value as SqlValue, Connection, OpenFlags};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::HashSet,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

const MANIFEST_SCHEMA: &str = "buglecat.verification.fixed-replay.v1";
const ROW_SCHEMA: &str = "buglecat.verification.prepared-replay.v1";
// Embedded at compilation, so check/test cannot masquerade as a rebuilt main.
const RUNNER_SOURCE: &[u8] = include_bytes!("verification_replay.rs");
const COLUMNS: &str = "event_id,session_id,task_id,agent_id,parent_agent_id,thread_id,turn_id,sequence,timestamp,event_type,status,tool_call_id,workspace,payload_json,source_ref";

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn value_sha(value: &Value) -> String {
    sha(value.to_string().as_bytes())
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("missing_{key}"))
}
fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|_| "source_read".into())
}
fn new_file(path: &Path) -> Result<File, String> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "output_exists_or_unwritable".into())
}
fn durable(file: &mut File, value: &Value) -> Result<(), String> {
    writeln!(file, "{value}").map_err(|_| "log_write")?;
    file.flush().map_err(|_| "log_flush")?;
    file.sync_all().map_err(|_| "log_sync".into())
}
fn sealed(mut value: Value) -> Value {
    value["artifact_sha256"] = json!(value_sha(&value));
    value
}
fn verify_seal(value: &Value) -> Result<(), String> {
    let mut body = value.clone();
    let expected = body
        .as_object_mut()
        .ok_or("row_object")?
        .remove("artifact_sha256")
        .ok_or("row_hash_missing")?;
    if expected != value_sha(&body) {
        return Err("row_hash_mismatch".into());
    }
    Ok(())
}

/// Copy a coherent read-only SQLite view, keeping original event sequences and
/// payloads. ContextStore may migrate/init only the disposable copy.
fn clone_events(
    source: &Path,
    destination: &Path,
    session_id: &str,
) -> Result<(ContextStore, String), String> {
    let origin = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "events_open")?;
    origin
        .execute_batch("BEGIN")
        .map_err(|_| "events_snapshot")?;
    let target = Connection::open(destination).map_err(|_| "events_copy")?;
    target.execute_batch("CREATE TABLE context_events(event_id TEXT PRIMARY KEY,session_id TEXT NOT NULL,task_id TEXT,agent_id TEXT,parent_agent_id TEXT,thread_id TEXT,turn_id INTEGER,sequence INTEGER NOT NULL,timestamp INTEGER NOT NULL,event_type TEXT NOT NULL,status TEXT NOT NULL,tool_call_id TEXT,workspace TEXT,payload_json TEXT NOT NULL,source_ref TEXT)").map_err(|_| "events_schema")?;
    let mut statement = origin
        .prepare(&format!(
            "SELECT {COLUMNS} FROM context_events WHERE session_id=?1 ORDER BY sequence"
        ))
        .map_err(|_| "events_query")?;
    let rows = statement
        .query_map([session_id], |row| {
            (0..15)
                .map(|i| row.get::<_, SqlValue>(i))
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|_| "events_rows")?;
    let mut snapshot = Sha256::new();
    for row in rows {
        let values = row.map_err(|_| "events_row")?;
        // Bind the typed values as read, before any current host interpretation.
        snapshot.update(format!("{values:?}\n").as_bytes());
        target.execute("INSERT INTO context_events VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)", params_from_iter(values)).map_err(|_| "events_insert")?;
    }
    drop(statement);
    origin
        .execute_batch("COMMIT")
        .map_err(|_| "events_commit")?;
    drop(target);
    Ok((
        ContextStore::open(destination).map_err(|_| "events_copy_open")?,
        format!("{:x}", snapshot.finalize()),
    ))
}

/// Mirrors private runtime external_state field-for-field, deriving every source
/// from the real host packet. No label or handmade scope participates.
fn single_state(input: &VerificationInput, claim: &Claim) -> Result<Value, String> {
    let sources: Vec<_> = claim.evidence_ids.iter().filter_map(|id| input.evidence.iter().find(|e| &e.id == id)).map(|e| {
        json!({"source_id":e.id,"status":e.status,"exit_code":e.exit_code,
            "command":e.command,"scope":e.scope,"excerpt":e.excerpt,"truncated":e.truncated,
            "file_freshness":match (&e.before,&e.after) {
                (Some(b),Some(a)) => b.complete && a.complete && input.current_files.complete && b.hashes == a.hashes && a.hashes == input.current_files.hashes,
                _ => false,
            }})
    }).collect();
    let raw = json!({"schema":"buglecat.jev.claim.v2","claim":claim.text,
        "candidate":input.candidate,"requirements":input.requirements,"evidence":sources,
        "coverage":input.coverage,"limitations":input.limitations})
    .to_string();
    if verification::evidence::unsafe_to_send(&raw) {
        return Err("sensitive_input".into());
    }
    serde_json::from_str(&verification::evidence::redact(&raw))
        .map_err(|_| "redaction_parse".into())
}

fn choose(input: &VerificationInput) -> Option<&Claim> {
    let mut claims: Vec<_> = input
        .claims
        .iter()
        .filter(|c| {
            c.kind == ClaimKind::Semantic && c.context == "narrative" && c.polarity == "positive"
        })
        .collect();
    claims.sort_by_key(|c| {
        let low = c.text.to_lowercase();
        let completion = [
            "completed",
            "finished",
            "resolved",
            "repaired",
            "已完成",
            "已修复",
        ]
        .iter()
        .any(|s| low.contains(s));
        (!completion, c.evidence_ids.is_empty(), c.start)
    });
    claims.first().copied()
}

fn prepare_case(case: &Value) -> Result<Value, String> {
    let id = text(case, "id")?;
    text(case, "group_id")?;
    let log = Path::new(text(case, "session_log")?)
        .canonicalize()
        .map_err(|_| "session_missing")?;
    let source_bytes = read(&log)?;
    if sha(&source_bytes) != text(case, "source_sha256")? {
        return Err("source_hash_mismatch".into());
    }
    let candidate = text(case, "candidate")?;
    if sha(candidate.as_bytes()) != text(case, "candidate_sha256")? {
        return Err("candidate_hash_mismatch".into());
    }
    let messages: Vec<Value> = String::from_utf8(source_bytes.clone())
        .map_err(|_| "session_utf8")?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|_| "session_parse")?;
    let original = messages.iter().any(|m| {
        m["role"] == "assistant" && verification::evidence::content_text(&m["content"]) == candidate
    });
    let projection =
        case["candidate_mode"] == "assertion_projection" && text(case, "projection_basis").is_ok();
    if !original && !projection {
        return Err("candidate_not_in_transcript".into());
    }
    let workspace = Path::new(text(case, "workspace")?)
        .canonicalize()
        .map_err(|_| "workspace_missing")?;
    if !workspace.is_dir() {
        return Err("workspace_not_directory".into());
    }
    let sources = log.with_extension("sources.jsonl");
    let trace_bytes = read(&sources).map_err(|_| "sources_missing")?;
    let traces: Vec<SourceRecord> = String::from_utf8(trace_bytes.clone())
        .map_err(|_| "sources_utf8")?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|_| "sources_parse")?;
    if traces.is_empty() {
        return Err("sources_empty".into());
    }
    let mut session = Session::resume("Offline provenance replay", Some(log.clone()));
    if traces.iter().any(|t| {
        matches!(
            t.kind,
            SourceKind::UserMessage
                | SourceKind::ToolCall
                | SourceKind::ToolResult
                | SourceKind::RuntimeVerification
        ) && t.session_id != session.session_id
    }) {
        return Err("sources_session_mismatch".into());
    }
    session.set_workspace(&workspace);
    let last_turn = traces
        .iter()
        .filter(|t| t.session_id == session.session_id)
        .filter_map(|t| t.turn_id)
        .max();
    if let Some(turn) = last_turn {
        session.set_turn_id(turn);
    }
    let temp = tempfile::tempdir().map_err(|_| "tempdir")?;
    let events_path = log.parent().ok_or("log_parent")?.join("events.sqlite3");
    let mut events_hash = None;
    let mut event_count = 0;
    if events_path.is_file() {
        let (store, hash) = clone_events(
            &events_path,
            &temp.path().join("events.sqlite3"),
            &session.session_id,
        )?;
        events_hash = Some(hash);
        event_count = store
            .session_events(&session.session_id)
            .map_err(|_| "events_read")?
            .len();
        session.set_event_store(Rc::new(store));
    }
    let packet =
        verification::evidence::packet(&session, &workspace, candidate, &Default::default());
    let rules = verification::policy::evaluate(&packet);
    let packet_value = serde_json::to_value(&packet).map_err(|_| "packet_serialize")?;
    let packet_hash = value_sha(&packet_value);
    let redacted_packet: Value =
        serde_json::from_str(&verification::evidence::redact(&packet_value.to_string()))
            .map_err(|_| "packet_redaction")?;
    let eligible = choose(&packet);
    let selected_decision =
        eligible.and_then(|c| rules.decisions.iter().find(|d| d.claim_id == c.id));
    let selected_evidence: Vec<_> = eligible.into_iter().flat_map(|c| &c.evidence_ids)
        .filter_map(|id| packet.evidence.iter().find(|e| &e.id == id))
        .map(|e| json!({"id":e.id,"source_ref":e.source_ref,"status":e.status,"exit_code":e.exit_code,
            "authoritative":e.authoritative,"scope":e.scope,"sequence":e.sequence,"superseded_by":e.superseded_by,
            "file_freshness":match (&e.before,&e.after) {
                (Some(b),Some(a)) => b.complete && a.complete && packet.current_files.complete && b == a && a == &packet.current_files,
                _ => false,
            }})).collect();
    let state = eligible.map(|c| single_state(&packet, c)).transpose()?;
    let request = state.clone().map(jev::request);
    if request
        .as_ref()
        .is_some_and(|r| r.to_string().len() > jev::MAX_INPUT_BYTES)
    {
        return Err("input_budget".into());
    }
    if read(&log)? != source_bytes || read(&sources)? != trace_bytes {
        return Err("source_changed_during_prepare".into());
    }
    Ok(
        json!({"schema":ROW_SCHEMA,"id":id,"group_id":case["group_id"],"status":"prepared",
        "candidate_mode":if projection {"assertion_projection"} else {"original_transcript"},
        "independent_blind":false,"review":{"label":case["label"],"review_status":case["review_status"],"label_basis":case["label_basis"],"split":case["split"],"author_exposed":case["author_exposed"],"source_kind":case["source_kind"]},"source_sha256":sha(&source_bytes),
        "sources_sha256":sha(&trace_bytes),"events_view_sha256":events_hash,
        "source_record_count":traces.len(),"event_count":event_count,
        "unattributed_source_records":traces.iter().filter(|t| t.session_id != session.session_id).count(),
        "workspace_identity_sha256":sha(workspace.to_string_lossy().as_bytes()),
        "candidate_sha256":sha(candidate.as_bytes()),"input_hash":packet_hash,
        "packet_redacted_sha256":value_sha(&redacted_packet),"packet":redacted_packet,
        "span_basis":"original candidate UTF-8 byte offsets",
        "host_B":rules,"eligible_semantic":eligible.is_some(),"selected_claim_id":eligible.map(|c| &c.id),
        "selected_claim_B":selected_decision,
        "selected_claim_summary":eligible.map(|c| json!({"id":c.id,"text":c.text,"start":c.start,"end":c.end,"kind":c.kind,"context":c.context,"polarity":c.polarity,"evidence":selected_evidence})),
        "request_hash":request.as_ref().map(value_sha),"request":request,"state":state,
        "policy_version":POLICY_VERSION,"template_version":TEMPLATE_VERSION,
        "requested_model":"jev-latest","production_authorized":false}),
    )
}

fn prepare(manifest: &Path, output: &Path) -> Result<(), String> {
    let bytes = read(manifest)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "manifest_parse")?;
    if value["schema"] != MANIFEST_SCHEMA {
        return Err("manifest_schema".into());
    }
    let cases = value["cases"].as_array().ok_or("missing_cases")?;
    let mut ids = HashSet::new();
    for case in cases {
        if !ids.insert(text(case, "id")?) {
            return Err("duplicate_case_id".into());
        }
    }
    let mut out = new_file(output)?;
    let mut eligible = 0;
    for case in cases {
        let mut row = prepare_case(case).unwrap_or_else(|error| json!({"schema":ROW_SCHEMA,"id":case["id"],"group_id":case["group_id"],"status":"not_run","reason":error,"eligible_semantic":false}));
        row["manifest_sha256"] = json!(sha(&bytes));
        eligible += usize::from(row["eligible_semantic"] == true);
        durable(&mut out, &sealed(row))?;
    }
    println!(
        "prepared_cases={} eligible_semantic={} paid_attempts=0 report_sha256={}",
        cases.len(),
        eligible,
        sha(&read(output)?)
    );
    Ok(())
}

struct RecordedTransport<'a> {
    inner: &'a dyn JevTransport,
    output: &'a RefCell<File>,
    ledger: &'a RefCell<File>,
    started: Value,
    request_hash: String,
    response_hash: RefCell<Option<String>>,
    response_path: PathBuf,
}
#[async_trait(?Send)]
impl JevTransport for RecordedTransport<'_> {
    async fn post(&self, key: &str, body: Value) -> Result<(u16, Vec<u8>), String> {
        if value_sha(&body) != self.request_hash {
            return Err("actual_request_hash_mismatch".into());
        }
        if self.response_path.exists() {
            return Err("raw_response_file_exists_before_request".into());
        }
        let mut submitted = self.started.clone();
        submitted["status"] = json!("request_submitted");
        submitted["actual_request_hash"] = json!(value_sha(&body));
        submitted["planned_raw_response_file"] = json!(self.response_path);
        durable(&mut self.ledger.borrow_mut(), &submitted)?;
        durable(&mut self.output.borrow_mut(), &submitted)?;
        let result = self.inner.post(key, body).await;
        if let Ok((status, bytes)) = &result {
            // HttpTransport intentionally omits non-2xx bodies; don't claim a
            // digest of an omitted API error body.
            if (200..300).contains(status) {
                if bytes.len() > jev::MAX_RESPONSE_BYTES {
                    return Err("response_budget".into());
                }
                let mut raw = new_file(&self.response_path)?;
                raw.write_all(bytes).map_err(|_| "raw_response_write")?;
                raw.flush().map_err(|_| "raw_response_flush")?;
                raw.sync_all().map_err(|_| "raw_response_sync")?;
                *self.response_hash.borrow_mut() = Some(sha(bytes));
            }
        }
        result
    }
}

async fn live(
    packets: &Path,
    output: &Path,
    ledger_dir: &Path,
    max: usize,
    cancel_file: Option<&Path>,
) -> Result<(), String> {
    if !(1..=25).contains(&max) {
        return Err("max_attempts_must_be_1_to_25".into());
    }
    let bytes = read(packets)?;
    let rows: Vec<Value> = String::from_utf8(bytes.clone())
        .map_err(|_| "packets_utf8")?
        .lines()
        .filter(|l| !l.is_empty())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|_| "packets_parse")?;
    let mut ids = HashSet::new();
    let mut eligible = vec![];
    let mut groups = HashSet::new();
    for row in &rows {
        verify_seal(row)?;
        if row["schema"] != ROW_SCHEMA {
            return Err("packet_schema".into());
        }
        if !ids.insert(text(row, "id")?) {
            return Err("duplicate_packet_id".into());
        }
        if row["eligible_semantic"] != true {
            continue;
        }
        if !groups.insert(text(row, "group_id")?) {
            return Err("only_one_live_case_per_group".into());
        }
        if row["selected_claim_B"]["verdict"] != "unknown" {
            return Err("selected_claim_not_unknown".into());
        }
        if row["policy_version"] != POLICY_VERSION || row["template_version"] != TEMPLATE_VERSION {
            return Err("packet_version_mismatch".into());
        }
        let expected = jev::request(row["state"].clone());
        if expected != row["request"] || row["request_hash"] != value_sha(&expected) {
            return Err("prepared_request_mismatch".into());
        }
        eligible.push(row);
    }
    if eligible.len() > max {
        return Err("eligible_cases_exceed_preregistered_attempt_budget".into());
    }
    let mut out = new_file(output)?;
    let key = std::env::var("NCX_JEV_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty());
    if key.is_none() || eligible.is_empty() {
        durable(
            &mut out,
            &json!({"status":"not_run","reason":if key.is_none() {"missing_key"} else {"no_eligible_semantic"},"attempted_calls":0}),
        )?;
        return Ok(());
    }
    let binary_path = std::env::current_exe().map_err(|_| "executable_path")?;
    let binary_hash = sha(&read(&binary_path)?);
    std::fs::create_dir_all(ledger_dir).map_err(|_| "ledger_directory")?;
    // One irreversible run claim per ledger directory, even if interrupted. A
    // pending started record counts; a new output name cannot retry the run.
    let mut claim = new_file(&ledger_dir.join("run.json"))?;
    durable(
        &mut claim,
        &json!({"packets_sha256":sha(&bytes),"max_attempts":max,"retry":0,"status":"claimed"}),
    )?;
    let output = RefCell::new(out);
    let cancel = || cancel_file.is_some_and(Path::exists);
    for (index, row) in eligible.iter().enumerate() {
        if cancel() {
            break;
        }
        let ledger = RefCell::new(new_file(
            &ledger_dir.join(format!("attempt-{:02}.jsonl", index + 1)),
        )?);
        let started = json!({"schema":"buglecat.verification.replay-attempt.v1","status":"attempt_started","attempt":index + 1,
            "id":row["id"],"group_id":row["group_id"],"request_hash":row["request_hash"],
            "candidate_hash":row["candidate_sha256"],"input_hash":row["input_hash"],
            "policy_version":POLICY_VERSION,"template_version":TEMPLATE_VERSION,"requested_model":"jev-latest",
            "runner_source_sha256":sha(RUNNER_SOURCE),"runner_exe_sha256":binary_hash});
        let transport = RecordedTransport {
            inner: &jev::HttpTransport,
            output: &output,
            ledger: &ledger,
            started: started.clone(),
            request_hash: text(row, "request_hash")?.into(),
            response_hash: RefCell::new(None),
            response_path: ledger_dir.join(format!("attempt-{:02}.raw-response.json", index + 1)),
        };
        durable(&mut ledger.borrow_mut(), &started)?;
        durable(&mut output.borrow_mut(), &started)?;
        let began = Instant::now();
        let result = jev::check_with(
            &transport,
            key.as_deref().unwrap(),
            row["state"].clone(),
            jev::TIMEOUT,
            Some(&cancel),
        )
        .await;
        let finished = match result {
            Ok(response) => {
                json!({"status":"attempt_finished","call_status":"ok","response":response,"model":response["model"],"usage":response["usage"]})
            }
            Err(error) => {
                json!({"status":"attempt_finished","call_status":error,"model":null,"usage":null})
            }
        };
        let mut completed = started;
        for (name, value) in finished.as_object().unwrap() {
            completed[name] = value.clone();
        }
        completed["response_sha256"] = json!(*transport.response_hash.borrow());
        completed["raw_response_file"] = if transport.response_hash.borrow().is_some() {
            json!(transport.response_path)
        } else {
            Value::Null
        };
        completed["elapsed_ms"] = json!(began.elapsed().as_millis());
        durable(&mut ledger.borrow_mut(), &completed)?;
        durable(&mut output.borrow_mut(), &completed)?;
    }
    Ok(())
}

struct OfflineReceiptTransport;
#[async_trait(?Send)]
impl JevTransport for OfflineReceiptTransport {
    async fn post(&self, _: &str, _: Value) -> Result<(u16, Vec<u8>), String> {
        let mut response = json!({"model":"offline-receipt-fixture","answers":{},"usage":{"input_tokens":0,"output_tokens":0}});
        for name in jev::QUESTIONS {
            response["answers"][name] = json!({"type":"noul","noul":0.0});
        }
        Ok((200, response.to_string().into_bytes()))
    }
}

/// Main-binary regression: no key lookup or HTTP transport is reachable here.
async fn offline_receipt_test(path: &Path) -> Result<(), String> {
    let output = RefCell::new(new_file(path)?);
    let ledger = RefCell::new(new_file(&path.with_extension("ledger.jsonl"))?);
    let state = json!({"schema":"buglecat.jev.claim.v2","claim":"Offline receipt fixture",
        "candidate":"Offline receipt fixture","requirements":[],"evidence":[],"coverage":"unknown","limitations":[]});
    let body = jev::request(state.clone());
    let binary = std::env::current_exe().map_err(|_| "executable_path")?;
    let started = json!({"status":"attempt_started","attempt":1,"request_hash":value_sha(&body),
        "offline":true,"network_calls":0,"paid_attempts":0,"policy_version":POLICY_VERSION,
        "runner_source_sha256":sha(RUNNER_SOURCE),"runner_exe_sha256":sha(&read(&binary)?),"request":body});
    durable(&mut ledger.borrow_mut(), &started)?;
    durable(&mut output.borrow_mut(), &started)?;
    let recorder = RecordedTransport {
        inner: &OfflineReceiptTransport,
        output: &output,
        ledger: &ledger,
        started: started.clone(),
        request_hash: value_sha(&body),
        response_hash: RefCell::new(None),
        response_path: path.with_extension("raw-response.json"),
    };
    let response = jev::check_with(
        &recorder,
        "offline-no-credential",
        state,
        jev::TIMEOUT,
        None,
    )
    .await?;
    let finished = json!({"status":"attempt_finished","offline":true,"network_calls":0,"paid_attempts":0,
        "call_status":"ok","response":response,"response_sha256":*recorder.response_hash.borrow(),
        "raw_response_file":recorder.response_path});
    durable(&mut ledger.borrow_mut(), &finished)?;
    durable(&mut output.borrow_mut(), &finished)?;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let result = if args.len() == 4 && args[1] == "prepare" {
        prepare(Path::new(&args[2]), Path::new(&args[3]))
    } else if args.len() == 3 && args[1] == "offline-receipt-test" {
        offline_receipt_test(Path::new(&args[2])).await
    } else if args.len() >= 9
        && args[1] == "live"
        && args[4] == "--live"
        && args[5] == "--max-attempts"
        && args[7] == "--ledger-dir"
    {
        let cancel = if args.len() == 11 && args[9] == "--cancel-file" {
            Some(PathBuf::from(&args[10]))
        } else {
            None
        };
        if args.len() != 9 && cancel.is_none() {
            Err("invalid_live_args".into())
        } else {
            live(
                Path::new(&args[2]),
                Path::new(&args[3]),
                Path::new(&args[8]),
                args[6].parse().unwrap_or(0),
                cancel.as_deref(),
            )
            .await
        }
    } else {
        Err("usage: verification_replay prepare <manifest> <new-packets.jsonl> | live <packets> <new-events.jsonl> --live --max-attempts N --ledger-dir <new-ledger-dir> [--cancel-file path]".into())
    };
    if let Err(error) = result {
        eprintln!("not_run: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncx_core::context_store::{ContextEvent, EventStatus, EventType};
    use std::cell::Cell;

    struct OfflineTransport {
        calls: Cell<usize>,
        ledger: PathBuf,
        slow: bool,
    }
    #[async_trait(?Send)]
    impl JevTransport for OfflineTransport {
        async fn post(&self, _: &str, _: Value) -> Result<(u16, Vec<u8>), String> {
            self.calls.set(self.calls.get() + 1);
            let persisted = std::fs::read_to_string(&self.ledger).unwrap();
            assert!(persisted.contains("attempt_started"));
            assert!(persisted.contains("request_submitted"));
            if self.slow {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            let mut response = json!({"model":"offline-fixture","answers":{},"usage":{"input_tokens":1,"output_tokens":2}});
            for name in jev::QUESTIONS {
                response["answers"][name] = json!({"type":"noul","noul":0.2});
            }
            Ok((200, response.to_string().into_bytes()))
        }
    }

    #[tokio::test]
    async fn actual_request_is_bound_and_started_precedes_transport_even_on_cancel() {
        for slow in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let ledger_path = temp.path().join("attempt.jsonl");
            let ledger = RefCell::new(new_file(&ledger_path).unwrap());
            let output = RefCell::new(new_file(&temp.path().join("events.jsonl")).unwrap());
            let state = json!({"claim":"untrusted positive assertion"});
            let started = json!({"status":"attempt_started","attempt":1,"request_hash":value_sha(&jev::request(state.clone()))});
            durable(&mut ledger.borrow_mut(), &started).unwrap();
            let fake = OfflineTransport {
                calls: Cell::new(0),
                ledger: ledger_path.clone(),
                slow,
            };
            let transport = RecordedTransport {
                inner: &fake,
                output: &output,
                ledger: &ledger,
                started,
                request_hash: value_sha(&jev::request(state.clone())),
                response_hash: RefCell::new(None),
                response_path: temp.path().join("raw-response.json"),
            };
            let cancelled = || slow && fake.calls.get() > 0;
            let result = jev::check_with(
                &transport,
                "never-print-key",
                state,
                jev::TIMEOUT,
                Some(&cancelled),
            )
            .await;
            assert_eq!(fake.calls.get(), 1);
            if slow {
                assert_eq!(result.unwrap_err(), "cancelled");
                assert!(transport.response_hash.borrow().is_none());
            } else {
                assert_eq!(result.unwrap()["model"], "offline-fixture");
                assert!(transport.response_hash.borrow().is_some());
                assert_eq!(
                    *transport.response_hash.borrow(),
                    Some(sha(&read(&transport.response_path).unwrap()))
                );
            }
            assert!(!std::fs::read_to_string(ledger_path)
                .unwrap()
                .contains("never-print-key"));
        }
    }

    #[tokio::test]
    async fn changed_body_never_reaches_transport() {
        let temp = tempfile::tempdir().unwrap();
        let ledger_path = temp.path().join("attempt.jsonl");
        let ledger = RefCell::new(new_file(&ledger_path).unwrap());
        let output = RefCell::new(new_file(&temp.path().join("events.jsonl")).unwrap());
        let fake = OfflineTransport {
            calls: Cell::new(0),
            ledger: ledger_path,
            slow: false,
        };
        let transport = RecordedTransport {
            inner: &fake,
            output: &output,
            ledger: &ledger,
            started: json!({}),
            request_hash: "wrong".into(),
            response_hash: RefCell::new(None),
            response_path: temp.path().join("raw-response.json"),
        };
        assert_eq!(
            jev::check_with(&transport, "fake-key", json!({}), jev::TIMEOUT, None)
                .await
                .unwrap_err(),
            "actual_request_hash_mismatch"
        );
        assert_eq!(fake.calls.get(), 0);
    }

    #[tokio::test]
    async fn error_response_is_not_saved_and_existing_raw_file_blocks_transport() {
        struct ErrorTransport;
        #[async_trait(?Send)]
        impl JevTransport for ErrorTransport {
            async fn post(&self, _key: &str, _body: Value) -> Result<(u16, Vec<u8>), String> {
                Ok((403, b"do-not-retain-provider-error-body".to_vec()))
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let ledger = RefCell::new(new_file(&temp.path().join("ledger.jsonl")).unwrap());
        let output = RefCell::new(new_file(&temp.path().join("events.jsonl")).unwrap());
        let state = json!({"claim":"fixture"});
        let path = temp.path().join("response.json");
        let recorder = RecordedTransport {
            inner: &ErrorTransport,
            output: &output,
            ledger: &ledger,
            started: json!({}),
            request_hash: value_sha(&jev::request(state.clone())),
            response_hash: RefCell::new(None),
            response_path: path.clone(),
        };
        assert_eq!(
            jev::check_with(&recorder, "offline", state.clone(), jev::TIMEOUT, None)
                .await
                .unwrap_err(),
            "http_403"
        );
        assert!(!path.exists());
        assert!(recorder.response_hash.borrow().is_none());
        std::fs::write(&path, b"original").unwrap();
        assert_eq!(
            jev::check_with(&recorder, "offline", state, jev::TIMEOUT, None)
                .await
                .unwrap_err(),
            "raw_response_file_exists_before_request"
        );
        assert_eq!(read(&path).unwrap(), b"original");
    }

    #[test]
    fn event_copy_preserves_sequence_without_modifying_source() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("original.sqlite3");
        let store = ContextStore::open(&source).unwrap();
        store
            .append(
                &ContextEvent::new(
                    "original",
                    "session",
                    EventType::UserMessage,
                    EventStatus::Completed,
                )
                .with_payload(&json!({"text":"Run check.py"})),
            )
            .unwrap();
        drop(store);
        let before = read(&source).unwrap();
        let (copy, _) =
            clone_events(&source, &temp.path().join("copy.sqlite3"), "session").unwrap();
        let event = copy.session_events("session").unwrap();
        assert_eq!(event[0].sequence, 1);
        assert_eq!(event[0].event_id, "original");
        assert_eq!(read(&source).unwrap(), before);
    }

    #[test]
    fn authentic_resume_and_projection_never_invent_sources() {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("case.jsonl");
        let mut original = Session::with_log("system", Some(log.clone()));
        original.set_workspace(temp.path());
        original.set_turn_id(1);
        original.add_user_text("Run python check.py");
        original.add_assistant("The requested feature is completed", None, "");
        let candidate = "The requested feature is completed";
        let case = json!({"id":"x","group_id":"x","session_log":log,"workspace":temp.path(),"candidate":candidate,"candidate_sha256":sha(candidate.as_bytes()),"source_sha256":sha(&read(&log).unwrap())});
        let row = prepare_case(&case).unwrap();
        assert_eq!(row["host_B"]["verdict"], "unknown");
        assert_eq!(row["eligible_semantic"], true);
        assert_eq!(
            row["request_hash"],
            value_sha(&jev::request(row["state"].clone()))
        );
        assert_eq!(row["packet"]["evidence"], json!([]));
        assert_eq!(row["state"]["evidence"], json!([]));
        original
            .trace_store()
            .unwrap()
            .append(&SourceRecord {
                source_id: "legacy-recovery".into(),
                kind: SourceKind::ToolRecovery,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            prepare_case(&case).unwrap()["unattributed_source_records"],
            1
        );
        let mut fake = case.clone();
        fake["candidate"] = json!("Other completed claim");
        fake["candidate_sha256"] = json!(sha(b"Other completed claim"));
        assert_eq!(
            prepare_case(&fake).unwrap_err(),
            "candidate_not_in_transcript"
        );
        fake["candidate_mode"] = json!("assertion_projection");
        fake["projection_basis"] = json!("Explicit offline projection; never blind");
        assert_eq!(
            prepare_case(&fake).unwrap()["candidate_mode"],
            "assertion_projection"
        );
        fake["source_sha256"] = json!("bad");
        assert_eq!(prepare_case(&fake).unwrap_err(), "source_hash_mismatch");
    }

    #[test]
    fn sealed_rows_and_outputs_are_immutable() {
        let mut row = sealed(json!({"id":"x","request_hash":"a"}));
        verify_seal(&row).unwrap();
        row["request_hash"] = json!("b");
        assert_eq!(verify_seal(&row).unwrap_err(), "row_hash_mismatch");
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("log");
        new_file(&path).unwrap();
        assert!(new_file(&path).is_err());
    }

    #[test]
    fn truthful_failure_reports_do_not_create_semantic_calls() {
        let session = Session::new("offline");
        let temp = tempfile::tempdir().unwrap();
        for candidate in [
            "The check failed with exit code 1.",
            "本轮校验未能确认完成声明；已保留工具记录和工作，请补充证据或明确修订回答。",
        ] {
            let packet = verification::evidence::packet(
                &session,
                temp.path(),
                candidate,
                &Default::default(),
            );
            assert!(choose(&packet).is_none(), "{candidate}");
        }
    }

    #[tokio::test]
    async fn live_rejects_group_variants_before_any_network_or_key_access() {
        let temp = tempfile::tempdir().unwrap();
        let packets = temp.path().join("packets.jsonl");
        let state = json!({"schema":"buglecat.jev.claim.v2","claim":"Completed"});
        let mut file = new_file(&packets).unwrap();
        for id in ["arm-A", "arm-B"] {
            durable(&mut file,&sealed(json!({"schema":ROW_SCHEMA,"id":id,"group_id":"same-original-task",
                "eligible_semantic":true,"selected_claim_B":{"verdict":"unknown"},
                "policy_version":POLICY_VERSION,"template_version":TEMPLATE_VERSION,
                "state":state,"request":jev::request(state.clone()),"request_hash":value_sha(&jev::request(state.clone()))}))).unwrap();
        }
        let output = temp.path().join("output.jsonl");
        assert_eq!(
            live(&packets, &output, &temp.path().join("ledger"), 25, None)
                .await
                .unwrap_err(),
            "only_one_live_case_per_group"
        );
        assert!(!output.exists());
        assert_eq!(
            live(&packets, &output, &temp.path().join("ledger"), 26, None)
                .await
                .unwrap_err(),
            "max_attempts_must_be_1_to_25"
        );
    }
}
