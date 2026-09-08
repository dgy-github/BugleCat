"""Versioned faults in real BugleCat owners. Never exported to a candidate."""
from dataclasses import dataclass


@dataclass(frozen=True)
class Case:
    id: str
    title: str
    target: str
    contract: str
    crate: str | None
    test_filter: str
    mutations: tuple[tuple[str, str], ...]


_CONTEXT = '''                // Context belongs to both sides of the hunk. Dropping it loses
                // insertion anchors and can silently modify the wrong function.
                let c = current.get_or_insert_with(|| Chunk {
                    locators: std::mem::take(&mut pending_locators),
                    ..Default::default()
                });
                c.context_lines.push((c.ins_lines.len(), c.del_lines.len()));
                c.del_lines.push(content.to_string());
                c.ins_lines.push(content.to_string());'''

CASES = (
    Case("s1_retry", "失败分类与重试边界", "bench/run.py",
         "Failed empty responses must retry with OR without missing files. Compile failure has "
         "priority; successful output mentioning empty response never retries. Preserve every "
         "physical invocation, logical attempt/retry indexes, all durations and timeouts. Zero "
         "disables retries; retain run_once's tuple API.", None, "",
         (("    compiles = evidence.get(\"python_compile\") or []",
           "    if evidence.get(\"missing_expected_files\"):\n        return \"missing_expected_file\"\n"
           "    compiles = evidence.get(\"python_compile\") or []"),
          ("times.append(total_elapsed)", "times.append(final_evidence.get(\"elapsed_s\", 0.0))"),
          ("                attempts.append(evidence)",
           "                if retry_no == 0:\n                    attempts.append(evidence)"))),
    Case("s2_patch", "补丁定位与缩进保留", "rust/crates/ncx-tools/src/patch.rs",
         "V4A space-prefixed context must locate the entire hunk. Insertion belongs inside its "
         "matched function, not at the file start. Context disambiguates repeated lines. Missing "
         "context must reject atomically. Whitespace fallback must preserve original context "
         "indentation; do not alter unchanged lines.", "ncx-tools", "patch::tests",
         ((_CONTEXT, "                flush!();"),)),
    Case("s3_budget", "超时预算与退出码", "rust/crates/ncx-cli/src/cli_app.rs",
         "The CLI may return zero only for completed. Budget exhaustion, cancellation, timeout, "
         "compile_failed, verification_failed and provider error are NOT success. Preserve the "
         "existing recorder and final output, and map incomplete reasons to a nonzero exit.",
         "ncx-cli", "incomplete_turns_return_failure",
         (("i32::from(reason != \"completed\")",
           "i32::from(matches!(reason, \"error\" | \"timeout\"))"),)),
    Case("s4_compile", "编译门禁与中断恢复", "rust/crates/ncx-core/src/tools/python_check.rs",
         "Native Python edits must not be accepted as complete if any tracked file has syntax "
         "errors or pending/interrupted/unavailable verification. Compile errors take precedence "
         "over other verification failures. After repair/recheck succeeds completion is allowed. "
         "Retain multi-file pending entries and current diagnostic evidence. Never execute the "
         "edited Python program merely to validate syntax.", "ncx-core", "python_",
         (("        Some(\"compile_failed\")", "        None // lost failure status"),
          ("        Some(\"verification_failed\")", "        None // lost pending status"))),
    Case("s5_encoding", "文件路径与编码兼容", "rust/crates/ncx-tools/src/text_encoding.rs",
         "Reading a UTF-16 LE BOM file through the real read_file tool must preserve Chinese "
         "text, even with Unicode, spaces and dot/parent path components. Correct BOM endianness "
         "must be selected. Preserve UTF-8/UTF-16 BE behavior and binary rejection.",
         "ncx-core", "self_repair_unicode_path",
         (("return decode_with(UTF_16LE, rest, TextEncoding::Utf16Le);",
           "return decode_with(UTF_16BE, rest, TextEncoding::Utf16Le);"),)),
    Case("s6_diagnostics", "连接诊断与日志脱敏",
         "rust/crates/ncx-provider/src/provider/diagnostics.rs",
         "Diagnostic JSON must never expose the configured credential, even if it appears in "
         "the model field. Retain model identity after redaction, HTTP status and elapsed time. "
         "retries counts attempts AFTER the initial call (first attempt -> zero retries). "
         "begin() must clear the previous HTTP status. Use synthetic credentials and no network.",
         "ncx-provider", "self_repair_diagnostic",
         (("model.replace(key, \"[REDACTED]\")", "model.to_string()"),
          ("self.attempts.saturating_sub(1)", "self.attempts"))),
)


def mutate(case: Case, original: str) -> str:
    for before, after in case.mutations:
        if original.count(before) != 1:
            raise ValueError(f"{case.id}: mutation anchor is absent or ambiguous; rebase the case")
        original = original.replace(before, after, 1)
    return original
