//! `read_file` line-numbered rendering — Rust port of the formatting logic in
//! `nanocodex/tools/read_file.py`.

const MAX_CHARS: usize = 100_000;
pub const DEFAULT_LIMIT: usize = 2000;

/// Render decoded file text as `LINE| TEXT`, honoring 1-indexed `offset` and
/// `limit`. Returns either the rendered window or an `Error:`/`(empty…)` string,
/// mirroring the Python tool's return values.
pub fn render(path: &str, text: &str, offset: usize, limit: usize) -> String {
    if text.is_empty() {
        return format!("(empty file: {path})");
    }
    let offset = offset.max(1);
    let limit = if limit == 0 { DEFAULT_LIMIT } else { limit };

    let normalized = text.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let total = lines.len();
    if offset > total {
        return format!("Error: offset {offset} is beyond end of file ({total} lines).");
    }

    let start = offset - 1;
    let end = (start + limit).min(total);
    let mut numbered: Vec<String> = Vec::with_capacity(end - start);
    for (i, ln) in lines[start..end].iter().enumerate() {
        numbered.push(format!("{}| {}", start + i + 1, ln));
    }
    let mut result = numbered.join("\n");
    if result.len() > MAX_CHARS {
        result.truncate(MAX_CHARS);
        result.push_str("\n... (truncated)");
    }
    if end < total {
        result.push_str(&format!(
            "\n\n(showing {offset}-{end} of {total} lines; offset={} to continue)",
            end + 1
        ));
    } else {
        result.push_str(&format!("\n\n(end of file — {total} lines)"));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file() {
        assert_eq!(render("a.txt", "", 1, 2000), "(empty file: a.txt)");
    }

    #[test]
    fn numbers_lines_from_one() {
        let out = render("a.txt", "alpha\nbeta\ngamma", 1, 2000);
        assert!(out.starts_with("1| alpha\n2| beta\n3| gamma"));
        assert!(out.contains("(end of file — 3 lines)"));
    }

    #[test]
    fn offset_and_limit_window() {
        let text = "l1\nl2\nl3\nl4\nl5";
        let out = render("a.txt", text, 2, 2);
        assert!(out.starts_with("2| l2\n3| l3"));
        assert!(out.contains("(showing 2-3 of 5 lines; offset=4 to continue)"));
    }

    #[test]
    fn offset_beyond_end_errors() {
        let out = render("a.txt", "one\ntwo", 5, 2000);
        assert_eq!(out, "Error: offset 5 is beyond end of file (2 lines).");
    }

    #[test]
    fn crlf_normalized() {
        let out = render("a.txt", "a\r\nb", 1, 2000);
        assert!(out.starts_with("1| a\n2| b"));
    }

    #[test]
    fn zero_offset_treated_as_one() {
        let out = render("a.txt", "x\ny", 0, 2000);
        assert!(out.starts_with("1| x"));
    }
}
