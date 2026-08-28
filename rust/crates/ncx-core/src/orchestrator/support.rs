use super::Complexity;

pub(super) const CLASSIFY_SYS: &str = "You are a task-complexity classifier. You have NO tools — do not attempt to read files or run commands. Reply with exactly one word — simple, medium, or high — rating how hard/risky the coding task is. simple = a one-step, low-risk change; medium = multi-step but routine; high = risky, broad, or easy to get wrong.";
pub(super) const PLAN_SYS: &str = "You are a senior engineer. You have NO tools and cannot read files — work only from the task text. Produce a short, concrete step-by-step plan to accomplish the task. Output the plan as plain text only — do not write code, do not call tools.";
pub(super) const DECOMPOSE_SYS: &str = "You are a planning lead. You have NO tools and cannot read files — work only from the task and plan text given. Break the task into the smallest set of INDEPENDENT subtasks that can be carried out one after another. Output ONLY subtask lines, each on its own line prefixed with 'SUBTASK: ' — no preamble, no prose, no tool calls. If the task is atomic (cannot be usefully split), output a single 'SUBTASK: ' line restating it.";
pub(super) const WORKER_SYS: &str = "You are an implementation worker. Carry out the task following the plan, using your tools. Describe what you did and the outcome.";
pub(super) const VERIFY_SYS: &str = "You are a strict reviewer. Given the task, plan, and the workers' results, decide whether the task is correctly and completely done. Start your reply with PASS or FAIL, then a one-line reason. If anything is wrong or missing, reply FAIL. On the LAST line, output 'BEST:<n>' giving the 1-based number of the worker whose result is best.";

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

pub(super) fn synthesize(results: &[String], best: usize, verdict: &str, passed: bool) -> String {
    let result = results
        .get(best)
        .or_else(|| results.first())
        .cloned()
        .unwrap_or_default();
    if passed {
        result
    } else {
        format!("{result}\n\n[unverified after retries — reviewer said: {verdict}]")
    }
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
