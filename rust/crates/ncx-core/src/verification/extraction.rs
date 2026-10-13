use super::types::*;
use regex::Regex;
use std::sync::OnceLock;

// Delimit prose only outside inline code and balanced argument lists. Byte
// positions are retained so downstream provenance spans remain exact.
fn protected_delimiters(line: &str) -> Vec<bool> {
    let mut protected = vec![false; line.len()];
    let mut brackets = Vec::new();
    let mut ticks = false;
    for (offset, ch) in line.char_indices() {
        if ch == '`' {
            ticks = !ticks;
        } else if !ticks {
            match ch {
                '(' | '[' | '{' => brackets.push(ch),
                ')' if brackets.last() == Some(&'(') => {
                    brackets.pop();
                }
                ']' if brackets.last() == Some(&'[') => {
                    brackets.pop();
                }
                '}' if brackets.last() == Some(&'{') => {
                    brackets.pop();
                }
                _ => {}
            }
        }
        protected[offset] = ticks || !brackets.is_empty();
    }
    protected
}

/// Explicit absence of execution, shared with policy so a truthful denial is
/// not mistaken for a success claim or for evidence that a failed run happened.
pub(super) fn denies_execution(text: &str) -> bool {
    static NO_TEST_EXECUTION: OnceLock<Regex> = OnceLock::new();
    let low = text.to_lowercase();
    NO_TEST_EXECUTION.get_or_init(|| {
        Regex::new(r"\bno\s+(?:(?:unit|integration)\s+)?tests?\s+(?:(?:were|was)\s+)?(?:run|executed)\b").unwrap()
    }).is_match(&low)
        || ["not run", "not executed", "did not run", "未运行", "未执行", "没有运行"]
            .iter().any(|phrase| low.contains(phrase))
}

pub fn claims(candidate: &str) -> Vec<Claim> {
    let target_pattern =
        Regex::new(r"[A-Za-z0-9_./\\-]+\.(?:rs|py|js|ts|tsx|json|toml|txt|md)").unwrap();
    let delimiters = Regex::new(r"[,，;；。!?！？，\n]|\b(?:but|while|and)\b|但是|而且").unwrap();
    // Explicit success denial; don't treat every occurrence of "not" as one.
    let denied_success = Regex::new(r"\bnot\s+(?:a\s+)?success\b").unwrap();
    let mut out = vec![];
    let mut fence = false;
    let mut line_offset = 0;
    for line in candidate.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = !fence;
        }
        let context = if fence || trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            "code"
        } else if trimmed.starts_with('>') || trimmed.starts_with(['“', '「', '"']) {
            "quoted"
        } else {
            "narrative"
        };
        let mut begin = 0;
        let protected = protected_delimiters(line);
        let ends: Vec<_> = delimiters
            .find_iter(line)
            .filter(|m| !protected[m.start()])
            .map(|m| (m.start(), m.end()))
            .chain(std::iter::once((line.len(), line.len())))
            .collect();
        for (end, next) in ends {
            let piece = &line[begin..end];
            let text = piece.trim();
            if !text.is_empty() {
                let low = text.to_lowercase();
                let negative = denied_success.is_match(&low)
                    || denies_execution(&low)
                    || ["not passed", "did not pass", "没通过", "failed", "失败"]
                        .iter()
                        .any(|s| low.contains(s));
                let limitation = [
                    "unverified",
                    "not verified",
                    "cannot confirm",
                    "无法确认",
                    "未验证",
                    "尚未",
                    "未完成",
                ]
                .iter()
                .any(|s| low.contains(s));
                let positive = [
                    "passed",
                    "success",
                    "completed",
                    "finished",
                    "implemented",
                    "fixed",
                    "repaired",
                    "resolved",
                    "已修复",
                    "已实现",
                    "ran ",
                    "executed",
                    "通过",
                    "成功",
                    "已完成",
                    "已运行",
                    "已执行",
                    "read ",
                    "wrote ",
                    "saved ",
                    "已读取",
                    "已保存",
                    "已修改",
                ]
                .iter()
                .any(|s| low.contains(s));
                let tests = [
                    "test",
                    "pytest",
                    "check passed",
                    "check failed",
                    "检查通过",
                    "检查失败",
                    "测试",
                    "单测",
                ]
                .iter()
                .any(|s| low.contains(s));
                let conditional = ["if ", "when ", "should ", "could ", "如果", "假设"]
                    .iter()
                    .any(|s| low.starts_with(s));
                let kind = if conditional || context != "narrative" || limitation {
                    ClaimKind::Limitation
                } else if (tests || !super::evidence::test_scope(&low).is_empty())
                    && (positive || negative)
                {
                    ClaimKind::TestsPassed
                } else if positive
                    && [
                        "ran ",
                        "executed",
                        "read ",
                        "wrote ",
                        "saved ",
                        "已运行",
                        "已执行",
                        "已读取",
                        "已保存",
                        "已修改",
                    ]
                    .iter()
                    .any(|s| low.contains(s))
                {
                    ClaimKind::ExecutionSuccess
                } else {
                    ClaimKind::Semantic
                };
                let scope = if ["all tests", "full suite", "全部测试", "全量"]
                    .iter()
                    .any(|s| low.contains(s))
                {
                    Some("all")
                } else if ["integration", "集成"].iter().any(|s| low.contains(s)) {
                    Some("integration")
                } else if ["unit", "单测", "--lib"].iter().any(|s| low.contains(s)) {
                    Some("unit")
                } else {
                    None
                };
                let hint = if low.contains("read ") || low.contains("读取") {
                    Some("read_file")
                } else if low.contains("wrote ")
                    || low.contains("saved ")
                    || low.contains("修改")
                    || low.contains("保存")
                {
                    Some("file_write")
                } else {
                    None
                };
                let target = target_pattern
                    .find(text)
                    .map(|m| m.as_str().replace('\\', "/"));
                let start = line_offset + begin + piece.find(text).unwrap();
                out.push(Claim {
                    id: format!("c{}", out.len() + 1),
                    text: text.into(),
                    start,
                    end: start + text.len(),
                    kind,
                    evidence_ids: vec![],
                    context: context.into(),
                    polarity: if negative {
                        "negative"
                    } else if positive {
                        "positive"
                    } else {
                        "uncertain"
                    }
                    .into(),
                    target,
                    requested_scope: scope.map(str::to_owned),
                    tool_hint: hint.map(str::to_owned),
                    binding_status: String::new(),
                });
            }
            begin = next;
        }
        line_offset += line.len();
    }
    out
}
