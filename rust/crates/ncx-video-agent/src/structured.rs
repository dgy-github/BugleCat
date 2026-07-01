use std::collections::{BTreeMap, HashSet};

use rusqlite::params;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::node::{
    assert_context_packet_admissible, AgentReasoningMode, ContextPacket, NodeKind, NodeSpec,
};
use crate::validation::{record_validation, ValidationInput};
use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentArtifactKind {
    Brief,
    Chapters,
    Shots,
    Assets,
}

impl AgentArtifactKind {
    pub fn stage(self) -> &'static str {
        match self {
            Self::Brief => "brief_self_check",
            Self::Chapters => "chapters_self_check",
            Self::Shots => "shots_self_check",
            Self::Assets => "assets_self_check",
        }
    }

    pub fn artifact_kind(self) -> &'static str {
        match self {
            Self::Brief => "brief",
            Self::Chapters => "chapters",
            Self::Shots => "storyboard",
            Self::Assets => "assets",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructuredValidationReport {
    pub kind: AgentArtifactKind,
    pub passed: bool,
    pub reasons: Vec<String>,
    pub metrics: Value,
}

impl StructuredValidationReport {
    fn pass(kind: AgentArtifactKind, metrics: Value) -> Self {
        Self {
            kind,
            passed: true,
            reasons: Vec::new(),
            metrics,
        }
    }

    fn repair(kind: AgentArtifactKind, reasons: Vec<String>, metrics: Value) -> Self {
        Self {
            kind,
            passed: false,
            reasons,
            metrics,
        }
    }
}

pub fn validate_brief_artifact(value: &Value) -> StructuredValidationReport {
    let mut reasons = Vec::new();
    let Some(obj) = value.as_object() else {
        return StructuredValidationReport::repair(
            AgentArtifactKind::Brief,
            vec!["brief artifact must be a JSON object".to_string()],
            json!({}),
        );
    };

    let summary = obj
        .get("brief")
        .or_else(|| obj.get("goal"))
        .or_else(|| obj.get("summary"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if summary.is_empty() {
        reasons.push("brief/goal/summary must be non-empty".to_string());
    }
    let duration_s = obj
        .get("duration_s")
        .and_then(Value::as_f64)
        .unwrap_or(-1.0);
    if duration_s <= 0.0 {
        reasons.push("duration_s must be positive".to_string());
    }
    let language = obj
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if language.is_empty() {
        reasons.push("language must be set".to_string());
    }

    let metrics = json!({
        "duration_s": positive_or_null(duration_s),
        "language": language,
    });
    finish(AgentArtifactKind::Brief, reasons, metrics)
}

pub fn validate_chapters_artifact(
    value: &Value,
    expected_duration_s: f64,
) -> StructuredValidationReport {
    let mut reasons = Vec::new();
    let chapters = value
        .get("chapters")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if chapters.is_empty() {
        reasons.push("chapters must be a non-empty array".to_string());
    }

    let mut ids = HashSet::new();
    let mut total = 0.0;
    for (idx, chapter) in chapters.iter().enumerate() {
        let prefix = format!("chapters[{idx}]");
        let Some(obj) = chapter.as_object() else {
            reasons.push(format!("{prefix} must be an object"));
            continue;
        };
        let id = required_string(obj.get("chapter_id"));
        if id.is_empty() {
            reasons.push(format!("{prefix}.chapter_id must be non-empty"));
        } else if !ids.insert(id.to_string()) {
            reasons.push(format!("duplicate chapter_id {id}"));
        }
        let duration = obj
            .get("duration_s")
            .and_then(Value::as_f64)
            .unwrap_or(-1.0);
        if duration <= 0.0 {
            reasons.push(format!("{prefix}.duration_s must be positive"));
        } else {
            total += duration;
        }
    }

    if expected_duration_s > 0.0 && (total - expected_duration_s).abs() > 0.001 {
        reasons.push(format!(
            "chapter duration mismatch: expected {expected_duration_s}, got {total}"
        ));
    }
    finish(
        AgentArtifactKind::Chapters,
        reasons,
        json!({"chapter_count": chapters.len(), "duration_s": total}),
    )
}

pub fn validate_shots_artifact(
    value: &Value,
    chapter_budgets: &BTreeMap<String, f64>,
) -> StructuredValidationReport {
    let mut reasons = Vec::new();
    let shots = value
        .get("shots")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if shots.is_empty() {
        reasons.push("shots must be a non-empty array".to_string());
    }

    let mut ids = HashSet::new();
    let mut per_chapter: BTreeMap<String, f64> = BTreeMap::new();
    let mut hero_count = 0usize;

    for (idx, shot) in shots.iter().enumerate() {
        let prefix = format!("shots[{idx}]");
        let Some(obj) = shot.as_object() else {
            reasons.push(format!("{prefix} must be an object"));
            continue;
        };
        let shot_id = required_string(obj.get("shot_id"));
        if shot_id.is_empty() {
            reasons.push(format!("{prefix}.shot_id must be non-empty"));
        } else if !ids.insert(shot_id.to_string()) {
            reasons.push(format!("duplicate shot_id {shot_id}"));
        }
    }

    for (idx, shot) in shots.iter().enumerate() {
        let prefix = format!("shots[{idx}]");
        let Some(obj) = shot.as_object() else {
            continue;
        };
        let shot_id = required_string(obj.get("shot_id"));
        let chapter_id = required_string(obj.get("chapter_id"));
        if chapter_id.is_empty() {
            reasons.push(format!("{prefix}.chapter_id must be non-empty"));
        } else if !chapter_budgets.contains_key(chapter_id) {
            reasons.push(format!("{prefix}.chapter_id {chapter_id} is not declared"));
        }
        let duration = obj
            .get("duration_s")
            .and_then(Value::as_f64)
            .unwrap_or(-1.0);
        if duration <= 0.0 {
            reasons.push(format!("{prefix}.duration_s must be positive"));
        } else if !chapter_id.is_empty() {
            *per_chapter.entry(chapter_id.to_string()).or_default() += duration;
        }
        let is_hero = obj.get("is_hero").and_then(Value::as_bool);
        if is_hero.is_none() {
            reasons.push(format!("{prefix}.is_hero must be a boolean"));
        } else if is_hero == Some(true) {
            hero_count += 1;
        }
        let tier = required_string(obj.get("tier"));
        if !matches!(tier, "hero" | "standard" | "filler") {
            reasons.push(format!("{prefix}.tier must be hero, standard, or filler"));
        }
        for field in ["continuity_in", "continuity_out"] {
            if let Some(reference) = obj.get(field).and_then(Value::as_str) {
                let reference = reference.trim();
                if !reference.is_empty()
                    && !is_boundary_reference(reference)
                    && !ids.contains(reference)
                {
                    reasons.push(format!(
                        "{prefix}.{field} references unknown shot {reference}"
                    ));
                }
            }
        }
        if shot_id.is_empty() {
            continue;
        }
    }

    for (chapter_id, expected) in chapter_budgets {
        let actual = per_chapter.get(chapter_id).copied().unwrap_or_default();
        if (actual - expected).abs() > 0.001 {
            reasons.push(format!(
                "shot duration mismatch for chapter {chapter_id}: expected {expected}, got {actual}"
            ));
        }
    }

    finish(
        AgentArtifactKind::Shots,
        reasons,
        json!({
            "shot_count": shots.len(),
            "hero_count": hero_count,
            "chapter_durations": per_chapter,
        }),
    )
}

pub fn validate_assets_artifact(
    value: &Value,
    shot_ids: &HashSet<String>,
) -> StructuredValidationReport {
    let mut reasons = Vec::new();
    let assets = value
        .get("assets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if assets.is_empty() {
        reasons.push("assets must be a non-empty array".to_string());
    }

    let mut ids = HashSet::new();
    for (idx, asset) in assets.iter().enumerate() {
        let prefix = format!("assets[{idx}]");
        let Some(obj) = asset.as_object() else {
            reasons.push(format!("{prefix} must be an object"));
            continue;
        };
        let id = required_string(obj.get("asset_id"));
        if id.is_empty() {
            reasons.push(format!("{prefix}.asset_id must be non-empty"));
        } else if !ids.insert(id.to_string()) {
            reasons.push(format!("duplicate asset_id {id}"));
        }
        if required_string(obj.get("type")).is_empty() {
            reasons.push(format!("{prefix}.type must be non-empty"));
        }
        let refs = obj
            .get("shot_ids")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if refs.is_empty() {
            reasons.push(format!("{prefix}.shot_ids must be non-empty"));
        }
        for reference in refs {
            let shot_id = reference.as_str().unwrap_or_default().trim();
            if shot_id.is_empty() || !shot_ids.contains(shot_id) {
                reasons.push(format!(
                    "{prefix}.shot_ids references unknown shot {shot_id}"
                ));
            }
        }
    }

    finish(
        AgentArtifactKind::Assets,
        reasons,
        json!({"asset_count": assets.len()}),
    )
}

pub fn record_structured_validation_if_pass(
    conn: &rusqlite::Connection,
    artifact_id: &str,
    gate_version: &str,
    report: &StructuredValidationReport,
) -> Result<bool> {
    if !report.passed {
        return Ok(false);
    }
    record_validation(
        conn,
        &ValidationInput {
            id: format!("validation_{}_{}", artifact_id, report.kind.stage()),
            artifact_id: artifact_id.to_string(),
            stage: report.kind.stage().to_string(),
            gate_version: gate_version.to_string(),
            verdict: "pass".to_string(),
            confidence: Some(1.0),
            aesthetic_score: None,
            layers_json: json!({
                "structured_validator": report.kind.stage(),
                "metrics": report.metrics,
            }),
            escalate_reason: None,
        },
    )?;
    Ok(true)
}

pub fn record_structured_agent_validation_if_pass(
    conn: &rusqlite::Connection,
    artifact_id: &str,
    gate_version: &str,
    report: &StructuredValidationReport,
    spec: &NodeSpec,
    packet: &ContextPacket,
) -> Result<bool> {
    assert_context_packet_admissible(conn, spec, packet)?;
    if !report.passed {
        return Ok(false);
    }
    record_validation(
        conn,
        &ValidationInput {
            id: format!("validation_{}_{}", artifact_id, report.kind.stage()),
            artifact_id: artifact_id.to_string(),
            stage: report.kind.stage().to_string(),
            gate_version: gate_version.to_string(),
            verdict: "pass".to_string(),
            confidence: Some(1.0),
            aesthetic_score: None,
            layers_json: json!({
                "structured_validator": report.kind.stage(),
                "metrics": report.metrics,
                "node_contract": {
                    "node_id": spec.node_id.as_str(),
                    "kind": node_kind_name(spec.kind),
                    "reasoning_mode": spec.reasoning_mode.map(reasoning_mode_name),
                    "tools": &spec.tools,
                    "judgment_or_planning": spec.is_judgment_or_planning,
                    "context_packet": {
                        "stage": packet.stage.as_str(),
                        "upstream_artifact_ids": &packet.upstream_artifact_ids,
                        "params": &packet.params_json,
                    },
                },
            }),
            escalate_reason: None,
        },
    )?;
    Ok(true)
}

pub fn json_content_hash(value: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_json(value).as_bytes());
    let digest = hasher.finalize();
    format!(
        "sha256:{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

pub fn chapter_budgets_from_artifact(value: &Value) -> BTreeMap<String, f64> {
    value
        .get("chapters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|chapter| {
            let id = chapter.get("chapter_id")?.as_str()?.trim();
            let duration = chapter.get("duration_s")?.as_f64()?;
            (!id.is_empty() && duration > 0.0).then(|| (id.to_string(), duration))
        })
        .collect()
}

pub fn shot_ids_from_artifact(value: &Value) -> HashSet<String> {
    value
        .get("shots")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|shot| shot.get("shot_id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub fn insert_project_artifact(
    conn: &rusqlite::Connection,
    id: &str,
    project_id: &str,
    kind: AgentArtifactKind,
    value: &Value,
) -> Result<()> {
    conn.execute(
        "INSERT INTO artifacts(id, project_id, shot_id, kind, tos_key, content_hash, params_json)
         VALUES(?1, ?2, NULL, ?3, ?4, ?5, ?6)",
        params![
            id,
            project_id,
            kind.artifact_kind(),
            format!("local://agent/{id}.json"),
            json_content_hash(value),
            canonical_json(value)
        ],
    )?;
    Ok(())
}

fn finish(
    kind: AgentArtifactKind,
    reasons: Vec<String>,
    metrics: Value,
) -> StructuredValidationReport {
    if reasons.is_empty() {
        StructuredValidationReport::pass(kind, metrics)
    } else {
        StructuredValidationReport::repair(kind, reasons, metrics)
    }
}

fn required_string(value: Option<&Value>) -> &str {
    value.and_then(Value::as_str).unwrap_or_default().trim()
}

fn is_boundary_reference(value: &str) -> bool {
    matches!(value, "start" | "end" | "none" | "null")
}

fn node_kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Agent => "agent",
        NodeKind::DeterministicTool => "deterministic_tool",
    }
}

fn reasoning_mode_name(mode: AgentReasoningMode) -> &'static str {
    match mode {
        AgentReasoningMode::SingleStructured => "single_structured",
        AgentReasoningMode::BoundedGenerateCritic => "bounded_generate_critic",
        AgentReasoningMode::BoundedReact => "bounded_react",
    }
}

fn positive_or_null(value: f64) -> Value {
    if value > 0.0 {
        json!(value)
    } else {
        Value::Null
    }
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => serde_json::to_string(v).expect("string serialization cannot fail"),
        Value::Array(items) => {
            let body = items
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{body}]")
        }
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            let body = keys
                .into_iter()
                .map(|key| {
                    let k = serde_json::to_string(key).expect("key serialization cannot fail");
                    let v = canonical_json(&map[key]);
                    format!("{k}:{v}")
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::db::Database;
    use crate::node::{p1_agent_node_spec, ContextPacket, P1AgentNode};
    use crate::test_support::temp_db_path;
    use crate::validation::assert_artifacts_passed;

    #[test]
    fn structured_chain_validates_brief_chapters_shots_and_assets() {
        let brief =
            json!({"brief": "make a concise product video", "duration_s": 6.0, "language": "zh"});
        assert!(validate_brief_artifact(&brief).passed);

        let chapters = json!({"chapters": [{"chapter_id": "chapter_01", "title": "Opening", "duration_s": 6.0}]});
        let chapter_report = validate_chapters_artifact(&chapters, 6.0);
        assert!(chapter_report.passed);
        let budgets = chapter_budgets_from_artifact(&chapters);

        let shots = json!({"shots": [
            {"shot_id": "shot_01", "chapter_id": "chapter_01", "duration_s": 3.0, "continuity_in": "start", "continuity_out": "shot_02", "is_hero": true, "tier": "hero"},
            {"shot_id": "shot_02", "chapter_id": "chapter_01", "duration_s": 3.0, "continuity_in": "shot_01", "continuity_out": "end", "is_hero": false, "tier": "standard"}
        ]});
        let shot_report = validate_shots_artifact(&shots, &budgets);
        assert!(shot_report.passed);

        let assets = json!({"assets": [
            {"asset_id": "studio", "type": "environment", "shot_ids": ["shot_01", "shot_02"]}
        ]});
        assert!(validate_assets_artifact(&assets, &shot_ids_from_artifact(&shots)).passed);
    }

    #[test]
    fn shots_validator_rejects_duration_reference_and_missing_routing_fields() {
        let budgets = BTreeMap::from([("chapter_01".to_string(), 4.0)]);
        let shots = json!({"shots": [
            {"shot_id": "shot_01", "chapter_id": "chapter_01", "duration_s": 2.0, "continuity_out": "missing", "tier": "unknown"}
        ]});
        let report = validate_shots_artifact(&shots, &budgets);
        assert!(!report.passed);
        assert!(report
            .reasons
            .iter()
            .any(|reason| reason.contains("is_hero")));
        assert!(report
            .reasons
            .iter()
            .any(|reason| reason.contains("unknown shot")));
        assert!(report
            .reasons
            .iter()
            .any(|reason| reason.contains("duration mismatch")));
    }

    #[test]
    fn invalid_artifact_does_not_get_a_pass_record() {
        let path = temp_db_path("structured-pass-record");
        let db = Database::open(&path).unwrap();
        db.create_project("p", 10.0).unwrap();
        db.create_chapter("c", "p", "{}").unwrap();
        db.create_scene("s", "c", "{}").unwrap();

        let bad = json!({"shots": []});
        insert_project_artifact(db.connection(), "a", "p", AgentArtifactKind::Shots, &bad).unwrap();
        let report = validate_shots_artifact(&bad, &BTreeMap::new());
        let wrote =
            record_structured_validation_if_pass(db.connection(), "a", "p1-test", &report).unwrap();
        assert!(!wrote);
        assert!(assert_artifacts_passed(db.connection(), &["a"]).is_err());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn agent_validation_records_context_packet_contract_evidence() {
        let path = temp_db_path("structured-node-contract");
        let db = Database::open(&path).unwrap();
        db.create_project("p", 10.0).unwrap();

        let brief = json!({"brief": "make it", "duration_s": 4.0, "language": "zh"});
        insert_project_artifact(
            db.connection(),
            "brief",
            "p",
            AgentArtifactKind::Brief,
            &brief,
        )
        .unwrap();
        let report = validate_brief_artifact(&brief);
        let packet = ContextPacket::new("brief", vec![], json!({"language": "zh"})).unwrap();
        let spec = p1_agent_node_spec(P1AgentNode::Requirements);
        record_structured_agent_validation_if_pass(
            db.connection(),
            "brief",
            "p1-test",
            &report,
            &spec,
            &packet,
        )
        .unwrap();

        let layers: String = db
            .connection()
            .query_row(
                "SELECT layers_json FROM validation_records WHERE artifact_id='brief'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let layers: Value = serde_json::from_str(&layers).unwrap();
        assert_eq!(
            layers["node_contract"]["reasoning_mode"],
            "single_structured"
        );
        assert_eq!(layers["node_contract"]["tools"], json!([]));
        assert_eq!(
            layers["node_contract"]["context_packet"]["params"]["language"],
            "zh"
        );

        let _ = std::fs::remove_file(path);
    }
}
