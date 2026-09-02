use super::*;

fn fixture(name: &str) -> PathBuf {
    let d = crate::test_support::unique_temp_dir(&format!("ncx_search_{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::create_dir_all(d.join("target")).unwrap(); // ignored
    std::fs::write(
        d.join("src/main.rs"),
        "fn main() {\n    let x = 42;\n    println!(\"hi\");\n}\n",
    )
    .unwrap();
    std::fs::write(d.join("src/util.rs"), "pub fn helper() -> i32 { 42 }\n").unwrap();
    std::fs::write(d.join("README.md"), "# Title\nsome TODO here\n").unwrap();
    std::fs::write(
        d.join("target/junk.rs"),
        "fn should_be_ignored() { let x = 42; }\n",
    )
    .unwrap();
    d
}

#[test]
fn glob_to_regex_matches_expected() {
    assert!(glob_to_regex("**/*.rs").is_match("src/main.rs"));
    assert!(glob_to_regex("**/*.rs").is_match("a/b/c.rs"));
    assert!(glob_to_regex("*.toml").is_match("Cargo.toml"));
    assert!(!glob_to_regex("*.toml").is_match("src/Cargo.toml")); // * doesn't cross /
    assert!(glob_to_regex("src/*.rs").is_match("src/main.rs"));
    assert!(!glob_to_regex("src/*.rs").is_match("src/sub/main.rs"));
}

#[test]
fn grep_finds_matches_and_skips_ignored() {
    let d = fixture("grep");
    let out = grep(&d, r"\b42\b", None, 200).unwrap();
    assert!(out.contains("src/main.rs:2"), "{out}");
    assert!(out.contains("src/util.rs:1"), "{out}");
    // target/ is ignored -> the junk file must not appear
    assert!(!out.contains("junk.rs"), "{out}");
}

#[test]
fn grep_path_glob_filters() {
    let d = fixture("grepglob");
    let out = grep(&d, "TODO", Some("**/*.md"), 200).unwrap();
    assert!(out.contains("README.md"), "{out}");
    let none = grep(&d, "TODO", Some("**/*.rs"), 200).unwrap();
    assert!(none.contains("No matches"), "{none}");
}

#[test]
fn grep_no_match_reports_count() {
    let d = fixture("nomatch");
    let out = grep(&d, "zzzznotfound", None, 200).unwrap();
    assert!(out.contains("No matches"));
}

#[test]
fn grep_invalid_regex_errors() {
    let d = fixture("badre");
    assert!(grep(&d, "(unclosed", None, 200).is_err());
}

#[test]
fn grep_literal_accepts_regex_metacharacters() {
    let d = fixture("literal");
    std::fs::write(d.join("src/literal.txt"), "value = [unfinished\n").unwrap();
    let out = grep_literal(&d, "[unfinished", None, 200).unwrap();
    assert!(out.contains("src/literal.txt:1"), "{out}");
}

#[test]
fn grep_finds_gb18030_chinese_text_in_deep_directory() {
    let d = fixture("gb18030");
    let nested = d.join("中文目录/第二层/第三层");
    std::fs::create_dir_all(&nested).unwrap();
    // “中文内容” encoded as GB18030/GBK, followed by a newline.
    std::fs::write(
        nested.join("旧编码.txt"),
        [0xD6, 0xD0, 0xCE, 0xC4, 0xC4, 0xDA, 0xC8, 0xDD, b'\n'],
    )
    .unwrap();

    let out = grep_literal(&d, "中文内容", None, 200).unwrap();

    assert!(out.contains("中文目录/第二层/第三层/旧编码.txt:1"), "{out}");
}

#[test]
fn grep_truncates_long_unicode_lines_without_panicking() {
    let d = fixture("unicode_truncate");
    std::fs::write(d.join("src/chinese.txt"), "中".repeat(400)).unwrap();

    let out = grep(&d, "中", None, 200).unwrap();

    assert!(out.contains("src/chinese.txt:1"), "{out}");
}

#[test]
fn glob_lists_rs_files_skipping_ignored() {
    let d = fixture("glob");
    let out = glob(&d, "**/*.rs", 200);
    assert!(out.contains("src/main.rs"));
    assert!(out.contains("src/util.rs"));
    assert!(!out.contains("junk.rs")); // target/ ignored
}

#[tokio::test]
async fn find_files_recurses_through_chinese_paths_and_skips_generated_dirs() {
    use ncx_sandbox::{SandboxPolicy, READ_ONLY};

    let d = fixture("find_files");
    std::fs::create_dir_all(d.join("中文目录/二级/三级")).unwrap();
    std::fs::create_dir_all(d.join(".nanocodex/checkpoints/snapshot")).unwrap();
    std::fs::write(d.join("中文目录/二级/三级/配置文件.toml"), "x=1").unwrap();
    std::fs::write(d.join("target/配置文件.toml"), "ignored=1").unwrap();
    std::fs::write(
        d.join(".nanocodex/checkpoints/snapshot/配置文件.toml"),
        "ignored=2",
    )
    .unwrap();
    let ctx = ToolContext::new(d.clone(), SandboxPolicy::new(READ_ONLY, &d));

    let out = FindFilesTool
        .execute(&ctx, &json!({"query": "配置文件.toml", "exact": true}))
        .await;

    assert!(out.contains("中文目录/二级/三级/配置文件.toml"), "{out}");
    assert!(!out.contains("target/配置文件.toml"), "{out}");
    assert!(!out.contains(".nanocodex"), "{out}");
}

#[tokio::test]
async fn find_files_reports_when_results_are_truncated() {
    use ncx_sandbox::{SandboxPolicy, READ_ONLY};

    let d = fixture("find_files_truncated");
    for index in 0..3 {
        std::fs::write(d.join(format!("src/match-{index}.txt")), "x").unwrap();
    }
    let ctx = ToolContext::new(d.clone(), SandboxPolicy::new(READ_ONLY, &d));

    let out = FindFilesTool
        .execute(&ctx, &json!({"query": "match-", "max_results": 2}))
        .await;
    let parsed: Value = serde_json::from_str(&out).unwrap();

    assert_eq!(parsed["count"], 2);
    assert_eq!(parsed["truncated"], true);
    assert_eq!(parsed["matches"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn web_tools_blocked_in_read_only() {
    use crate::tools::ToolContext;
    use ncx_sandbox::{SandboxPolicy, READ_ONLY};
    let ws = std::env::temp_dir();
    let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(READ_ONLY, &ws));
    let s = WebSearchTool.execute(&ctx, &json!({ "query": "x" })).await;
    assert!(s.contains("disabled") && s.contains("read-only"), "{s}");
    let f = WebFetchTool
        .execute(&ctx, &json!({ "url": "http://example.com" }))
        .await;
    assert!(f.contains("disabled") && f.contains("read-only"), "{f}");
}
