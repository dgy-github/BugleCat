//! `@path` file-mention expansion — Rust port of `nanocodex/agent/mentions.py`.
//!
//! Typing `@src/foo.py` in a message pulls that file's contents inline so the
//! model sees it without a separate read_file round-trip. Pure and offline: a
//! `@token` that doesn't resolve to a readable UTF-8 file is left untouched, so
//! `@channel`, e-mail addresses, and decorators are never mangled.

use std::path::{Path, PathBuf};

const TRIM_TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '\'', '"', '`'];
const MAX_FILE_BYTES: usize = 50_000;
const MAX_FILES: usize = 10;
const MAX_TOTAL_BYTES: usize = 200_000;

/// Return the `@`-mention path tokens in order (trailing punctuation trimmed).
///
/// A mention is `@` at line start or after whitespace, followed by a run of
/// non-space, non-`@` characters.
pub fn find_mentions(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' {
            let at_boundary = i == 0 || chars[i - 1].is_whitespace();
            if at_boundary {
                let mut j = i + 1;
                while j < chars.len() && !chars[j].is_whitespace() && chars[j] != '@' {
                    j += 1;
                }
                let raw: String = chars[i + 1..j].iter().collect();
                let tok = raw.trim_end_matches(TRIM_TRAILING).to_string();
                if !tok.is_empty() {
                    out.push(tok);
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Append inline file context for each `@mention` resolving to a readable file.
///
/// The original text is preserved; one `<file path="…">` block per resolved file
/// is appended. Files are de-duplicated and capped in count and total size.
/// Returns the text unchanged when nothing resolves.
pub fn expand_file_mentions(text: &str, workspace: &Path) -> String {
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut blocks: Vec<String> = Vec::new();
    let mut total = 0usize;

    for tok in find_mentions(text) {
        if seen.len() >= MAX_FILES {
            break;
        }
        let p = Path::new(&tok);
        let abs = if p.is_absolute() { p.to_path_buf() } else { workspace.join(&tok) };
        let resolved = abs.canonicalize().unwrap_or(abs);
        if seen.contains(&resolved) || !resolved.is_file() {
            continue;
        }
        let Ok(data) = std::fs::read(&resolved) else { continue };
        if data.is_empty() {
            continue;
        }
        let truncated = data.len() > MAX_FILE_BYTES;
        let slice = if truncated { &data[..MAX_FILE_BYTES] } else { &data[..] };
        if total + slice.len() > MAX_TOTAL_BYTES {
            break;
        }
        let Ok(content) = std::str::from_utf8(slice) else { continue };
        seen.push(resolved);
        total += slice.len();
        let suffix = if truncated { "\n... (truncated)" } else { "" };
        blocks.push(format!("<file path=\"{tok}\">\n{content}{suffix}\n</file>"));
    }

    if blocks.is_empty() {
        return text.to_string();
    }
    format!("{text}\n\n{}", blocks.join("\n\n"))
}

// ── tests (mirror tests/test_mentions.py) ─────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ncx_mentions_{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn find_mentions_basic_and_trailing_punct() {
        assert_eq!(find_mentions("look at @src/a.py and @b.txt."), vec!["src/a.py", "b.txt"]);
        assert_eq!(find_mentions("mail me@example.com"), Vec::<String>::new());
    }

    #[test]
    fn expand_inlines_existing_file() {
        let d = tmpdir("inline");
        std::fs::write(d.join("hello.py"), "print('hi')\n").unwrap();
        let out = expand_file_mentions("explain @hello.py please", &d);
        assert!(out.contains("explain @hello.py please"));
        assert!(out.contains("<file path=\"hello.py\">"));
        assert!(out.contains("print('hi')"));
    }

    #[test]
    fn nonexistent_mention_is_left_alone() {
        let d = tmpdir("nope");
        let out = expand_file_mentions("see @nope.py", &d);
        assert_eq!(out, "see @nope.py");
    }

    #[test]
    fn dedup_and_multiple() {
        let d = tmpdir("dedup");
        std::fs::write(d.join("a.txt"), "AAA").unwrap();
        std::fs::write(d.join("b.txt"), "BBB").unwrap();
        let out = expand_file_mentions("@a.txt @b.txt @a.txt", &d);
        assert_eq!(out.matches("<file path=\"a.txt\">").count(), 1);
        assert!(out.contains("<file path=\"b.txt\">"));
    }

    #[test]
    fn binary_file_skipped() {
        let d = tmpdir("binary");
        std::fs::write(d.join("img.bin"), [0xff, 0xfe, 0x00, 0x01, 0x02]).unwrap();
        let out = expand_file_mentions("@img.bin", &d);
        assert_eq!(out, "@img.bin");
    }

    #[test]
    fn large_file_truncated() {
        let d = tmpdir("large");
        std::fs::write(d.join("big.txt"), "x".repeat(60_000)).unwrap();
        let out = expand_file_mentions("@big.txt", &d);
        assert!(out.contains("... (truncated)"));
    }
}
