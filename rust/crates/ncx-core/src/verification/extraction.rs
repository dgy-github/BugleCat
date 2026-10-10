use super::types::*;
use regex::Regex;

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
        let ends: Vec<_> = delimiters
            .find_iter(line)
            .map(|m| (m.start(), m.end()))
            .chain(std::iter::once((line.len(), line.len())))
            .collect();
        for (end, next) in ends {
            let piece = &line[begin..end];
            let text = piece.trim();
            if !text.is_empty() {
                let low = text.to_lowercase();
                let negative = denied_success.is_match(&low)
                    || [
                        "not passed",
                        "did not pass",
                        "没通过",
                        "not run",
                        "not executed",
                        "未运行",
                        "未执行",
                        "did not run",
                        "没有运行",
                        "failed",
                        "失败",
                    ]
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
                } else if tests && (positive || negative) {
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
