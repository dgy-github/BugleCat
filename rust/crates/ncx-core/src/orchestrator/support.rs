use super::Complexity;

pub const CLASSIFY_SYS: &str = "You are a task-complexity classifier. You have NO tools — do not attempt to read files or run commands. Reply with exactly one word — simple, medium, or high — rating how hard/risky the coding task is. simple = a one-step, low-risk change; medium = multi-step but routine; high = risky, broad, or easy to get wrong.";
pub const PLAN_SYS: &str = "You are a senior engineer. You have NO tools and cannot read files — work only from the task text. Produce a short, concrete step-by-step plan to accomplish the task. Output the plan as plain text only — do not write code, do not call tools.";
pub const DECOMPOSE_SYS: &str = "You are a planning lead. You have NO tools and cannot read files — work only from the task and plan text given. Break the task into the smallest set of INDEPENDENT subtasks that can be carried out one after another. Output ONLY subtask lines, each on its own line prefixed with 'SUBTASK: ' — no preamble, no prose, no tool calls. If the task is atomic (cannot be usefully split), output a single 'SUBTASK: ' line restating it.";
pub const WORKER_SYS: &str = "You are an implementation worker. Carry out the task following the plan, using your tools. Describe what you did and the outcome.";
pub const VERIFY_SYS: &str = "You are a strict reviewer. Given the task, plan, and the workers' results, decide whether the task is correctly and completely done. Start your reply with PASS or FAIL, then a one-line reason. If anything is wrong or missing, reply FAIL. Then output 'BEST:<n>' on its own line, giving the 1-based number of the worker whose result is best. Only if other workers made complementary changes to DIFFERENT files that the best worker lacks and that belong in the final result, add one last line 'COMPOSE:<best>,<n>,...' listing the best worker first and then those workers; otherwise omit the COMPOSE line.";
pub const COMPOSE_VERIFY_SYS: &str = "You are a strict reviewer checking a MERGED workspace: several workers attempted the same task in isolation, and their file changes were combined into the current directory (where two workers changed the same file, the first-listed worker's version was kept). Use your tools to read the changed files and run the relevant build or tests. Do not modify any files. Decide whether the merged result correctly and completely does the task and whether the combined changes fit together (no duplicate definitions, double-applied fixes, or broken imports). Start your reply with PASS or FAIL, then a one-line reason.";

pub(super) fn orch_trace(message: &str) {
    if std::env::var_os("NCX_TRACE").is_some_and(|value| !value.is_empty()) {
        eprintln!("[ncx-trace][orch] {message}");
    }
}

pub(super) fn parse_complexity(value: &str) -> Complexity {
    let value = value.to_lowercase();
    if value.contains("high") {
        Complexity::High
    } else if value.contains("simple") {
        Complexity::Simple
    } else {
        Complexity::Medium
    }
}

pub(super) fn verdict_passed(verdict: &str) -> bool {
    !verdict.to_uppercase().contains("FAIL")
}

pub(super) fn parse_subtasks(value: &str) -> Vec<String> {
    let mut items = value
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let position = line.to_uppercase().find("SUBTASK:")?;
            let item = line[position + "SUBTASK:".len()..].trim();
            (!item.is_empty()).then(|| item.to_string())
        })
        .collect::<Vec<_>>();
    if items.is_empty() {
        items = value
            .lines()
            .filter_map(|line| strip_list_marker(line.trim()).map(str::to_string))
            .collect();
    }
    items
}

fn strip_list_marker(line: &str) -> Option<&str> {
    if let Some(rest) = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("• "))
    {
        return Some(rest.trim());
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    line.get(digits..)?
        .strip_prefix('.')
        .or_else(|| line.get(digits..)?.strip_prefix(')'))
        .map(str::trim)
}

pub(super) fn build_worker_task(
    task: &str,
    plan: &str,
    feedback: &str,
    index: usize,
    count: usize,
) -> String {
    let mut value = format!(
        "Task:\n{task}\n\nPlan:\n{plan}\n\n(You are worker {} of {}.)",
        index + 1,
        count
    );
    if !feedback.is_empty() {
        value.push_str(&format!(
            "\n\nThe previous attempt was rejected. Address this feedback:\n{feedback}"
        ));
    }
    value
}

pub(super) fn build_decompose_task(task: &str, plan: &str) -> String {
    format!("Task:\n{task}\n\nPlan:\n{plan}")
}

pub(super) fn build_verify_task(task: &str, plan: &str, results: &[String]) -> String {
    let joined = results
        .iter()
        .enumerate()
        .map(|(index, result)| format!("--- worker {} ---\n{result}", index + 1))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("Task:\n{task}\n\nPlan:\n{plan}\n\nWorker results:\n{joined}")
}

/// Task for [`COMPOSE_VERIFY_SYS`]: which workers were merged (primary first)
/// and what each of them reported doing.
pub(super) fn build_compose_verify_task(
    task: &str,
    plan: &str,
    results: &[String],
    composed: &[usize],
) -> String {
    let labels = worker_labels(composed, ", ");
    let reports = composed
        .iter()
        .filter_map(|&idx| {
            results
                .get(idx)
                .map(|text| format!("--- worker {} ---\n{text}", idx + 1))
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "Task:\n{task}\n\nPlan:\n{plan}\n\nThe current directory holds the merged changes of workers {labels} \
         (listed by priority; any of them whose edits clashed with an earlier one were left out).\n\n\
         Their reports:\n{reports}"
    )
}

/// 1-based worker numbers joined by `sep`.
pub(super) fn worker_labels(indices: &[usize], sep: &str) -> String {
    indices
        .iter()
        .map(|idx| (idx + 1).to_string())
        .collect::<Vec<_>>()
        .join(sep)
}

pub(super) fn synthesize(results: &[String], best: usize, verdict: &str, passed: bool) -> String {
    synthesize_composed(results, &[best], verdict, passed)
}

pub(super) fn synthesize_composed(
    results: &[String],
    composed: &[usize],
    verdict: &str,
    passed: bool,
) -> String {
    let best = composed.first().copied().unwrap_or(0);
    let mut result = results
        .get(best)
        .or_else(|| results.first())
        .cloned()
        .unwrap_or_default();
    if composed.len() > 1 {
        let extras = composed
            .iter()
            .skip(1)
            .filter_map(|&idx| {
                results
                    .get(idx)
                    .map(|text| format!("[Composed from Worker {}]\n{text}", idx + 1))
            })
            .collect::<Vec<_>>();
        if !extras.is_empty() {
            result = format!("{result}\n\n{}", extras.join("\n\n"));
        }
    }
    if passed {
        result
    } else {
        format!("{result}\n\n[unverified after retries — reviewer said: {verdict}]")
    }
}

/// Harness² Propose–Probe–Compose: when a multi-worker round fails verification,
/// combine the reviewer verdict with a bounded digest of all workers' probe
/// findings so the next round can compose complementary insights across workers.
pub(super) fn build_compose_feedback(results: &[String], verdict: &str) -> String {
    if results.len() < 2 {
        return verdict.to_string();
    }
    let mut peer_digest = Vec::with_capacity(results.len());
    for (idx, text) in results.iter().enumerate() {
        let snippet: String = text.trim().chars().take(240).collect();
        if !snippet.is_empty() {
            peer_digest.push(format!("- Worker {}: {snippet}", idx + 1));
        }
    }
    if peer_digest.is_empty() {
        return verdict.to_string();
    }
    format!(
        "{verdict}\n\n[Propose-Probe-Compose Peer Digest — combine complementary findings and avoid repeated traps]:\n{}",
        peer_digest.join("\n")
    )
}

pub(super) fn synthesize_subtasks(results: &[String], verdict: &str, passed: bool) -> String {
    let body = results.join("\n\n");
    if passed {
        body
    } else {
        format!("{body}\n\n[unverified after decomposition — reviewer said: {verdict}]")
    }
}

pub(super) fn parse_best_worker(verdict: &str, count: usize) -> usize {
    let index = verdict
        .to_uppercase()
        .find("BEST:")
        .and_then(|position| {
            verdict[position + 5..]
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<usize>()
                .ok()
        })
        .map(|value| value.saturating_sub(1))
        .unwrap_or(0);
    index.min(count.saturating_sub(1))
}

/// Parse optional `COMPOSE:<n1>,<n2>...` (1-based worker numbers) from verifier
/// output. Always returns `best` as the first element, followed by any valid
/// distinct secondary worker indices `< count`; out-of-range numbers are
/// ignored rather than clamped onto a worker the verifier never named.
pub(super) fn parse_compose_workers(verdict: &str, count: usize, best: usize) -> Vec<usize> {
    let mut indices = vec![best.min(count.saturating_sub(1))];
    if count <= 1 {
        return indices;
    }
    let upper = verdict.to_uppercase();
    if let Some(pos) = upper.find("COMPOSE:") {
        let tail = &verdict[pos + "COMPOSE:".len()..];
        let line = tail.lines().next().unwrap_or("");
        for token in line.split(|c: char| c == ',' || c == '+' || c.is_whitespace()) {
            let digits: String = token.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(one_based) = digits.parse::<usize>() {
                if (1..=count).contains(&one_based) {
                    let zero_based = one_based - 1;
                    if !indices.contains(&zero_based) {
                        indices.push(zero_based);
                    }
                }
            }
        }
    }
    indices
}
