use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use fasttext_pure_rs::FastText;
use rusqlite::params;
use serde_json::Value;

use crate::Result;

pub trait LanguageDetector {
    fn detect_language(&self, text: &str) -> Result<String>;
}

#[derive(Debug, Clone)]
pub struct FastTextCliDetector {
    model_path: PathBuf,
    binary: PathBuf,
}

impl FastTextCliDetector {
    pub fn new(model_path: impl Into<PathBuf>) -> Self {
        Self {
            model_path: model_path.into(),
            binary: PathBuf::from("fasttext"),
        }
    }

    pub fn with_binary(model_path: impl Into<PathBuf>, binary: impl Into<PathBuf>) -> Self {
        Self {
            model_path: model_path.into(),
            binary: binary.into(),
        }
    }

    pub fn model_path(&self) -> &Path {
        &self.model_path
    }
}

impl LanguageDetector for FastTextCliDetector {
    fn detect_language(&self, text: &str) -> Result<String> {
        let mut child = Command::new(&self.binary)
            .args(["predict"])
            .arg(&self.model_path)
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                crate::VideoAgentError::L0Rejected(format!(
                    "failed to launch fastText CLI {}: {err}",
                    self.binary.display()
                ))
            })?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(format!("{text}\n").as_bytes())
                .map_err(|err| {
                    crate::VideoAgentError::L0Rejected(format!(
                        "write fastText stdin failed: {err}"
                    ))
                })?;
        }

        let output = child.wait_with_output().map_err(|err| {
            crate::VideoAgentError::L0Rejected(format!("fastText predict failed: {err}"))
        })?;
        if !output.status.success() {
            return Err(crate::VideoAgentError::L0Rejected(format!(
                "fastText exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        parse_fasttext_label(&String::from_utf8_lossy(&output.stdout)).ok_or_else(|| {
            crate::VideoAgentError::L0Rejected(format!(
                "fastText output did not contain a language label: {}",
                String::from_utf8_lossy(&output.stdout).trim()
            ))
        })
    }
}

#[derive(Debug)]
pub struct FastTextModelDetector {
    model_path: PathBuf,
    model: FastText,
}

impl FastTextModelDetector {
    pub fn load(model_path: impl Into<PathBuf>) -> Result<Self> {
        let model_path = model_path.into();
        let model = FastText::load(&model_path).map_err(|err| {
            crate::VideoAgentError::L0Rejected(format!(
                "load fastText model {} failed: {err}",
                model_path.display()
            ))
        })?;
        Ok(Self { model_path, model })
    }

    pub fn model_path(&self) -> &Path {
        &self.model_path
    }
}

impl LanguageDetector for FastTextModelDetector {
    fn detect_language(&self, text: &str) -> Result<String> {
        let predictions = self.model.predict(text, 1, 0.0).map_err(|err| {
            crate::VideoAgentError::L0Rejected(format!(
                "fastText model prediction failed for {}: {err}",
                self.model_path.display()
            ))
        })?;
        let Some(prediction) = predictions.first() else {
            return Err(crate::VideoAgentError::L0Rejected(format!(
                "fastText model {} returned no language labels",
                self.model_path.display()
            )));
        };
        prediction
            .label
            .strip_prefix("__label__")
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                crate::VideoAgentError::L0Rejected(format!(
                    "fastText model returned malformed language label: {}",
                    prediction.label
                ))
            })
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct HeuristicLanguageDetector;

impl LanguageDetector for HeuristicLanguageDetector {
    fn detect_language(&self, text: &str) -> Result<String> {
        if text.chars().any(is_cjk) {
            Ok("zh".to_string())
        } else if text.chars().any(|ch| ch.is_ascii_alphabetic()) {
            Ok("en".to_string())
        } else {
            Ok("unknown".to_string())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum L0Verdict {
    Pass,
    Repair,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L0Report {
    pub verdict: L0Verdict,
    pub reasons: Vec<String>,
}

impl L0Report {
    fn pass() -> Self {
        Self {
            verdict: L0Verdict::Pass,
            reasons: vec![],
        }
    }

    fn repair(reasons: Vec<String>) -> Self {
        Self {
            verdict: L0Verdict::Repair,
            reasons,
        }
    }
}

pub fn validate_scene_l0(
    conn: &rusqlite::Connection,
    scene_id: &str,
    expected_duration_s: f64,
    language_detector: &dyn LanguageDetector,
) -> Result<L0Report> {
    let mut stmt = conn.prepare(
        "SELECT id, plan_json, continuity_in, continuity_out
         FROM shots
         WHERE scene_id=?1
         ORDER BY id",
    )?;
    let rows = stmt.query_map(params![scene_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;

    let mut shots = Vec::new();
    for row in rows {
        let (id, plan_json, continuity_in, continuity_out) = row?;
        let plan: Value = serde_json::from_str(&plan_json)?;
        shots.push(ShotForL0 {
            id,
            plan,
            continuity_in,
            continuity_out,
        });
    }

    let mut reasons = Vec::new();
    if shots.is_empty() {
        reasons.push("scene has no shots".to_string());
    }

    let ids = shots
        .iter()
        .map(|shot| shot.id.as_str())
        .collect::<HashSet<_>>();
    let mut total_duration = 0.0;

    for shot in &shots {
        let duration = shot
            .plan
            .get("duration_s")
            .and_then(Value::as_f64)
            .unwrap_or(-1.0);
        if duration <= 0.0 {
            reasons.push(format!("shot {} has invalid duration_s", shot.id));
        } else {
            total_duration += duration;
        }

        for (field, value) in [
            ("continuity_in", shot.continuity_in.as_deref()),
            ("continuity_out", shot.continuity_out.as_deref()),
            (
                "plan.continuity_in",
                shot.plan.get("continuity_in").and_then(Value::as_str),
            ),
            (
                "plan.continuity_out",
                shot.plan.get("continuity_out").and_then(Value::as_str),
            ),
        ] {
            if let Some(reference) = value.map(str::trim).filter(|v| !v.is_empty()) {
                if !is_boundary_reference(reference) && !ids.contains(reference) {
                    reasons.push(format!(
                        "shot {} has unclosed {field} reference {reference}",
                        shot.id
                    ));
                }
            }
        }

        let requires_chinese = shot
            .plan
            .get("requires_chinese")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || shot
                .plan
                .get("required_language")
                .and_then(Value::as_str)
                .is_some_and(|lang| lang.eq_ignore_ascii_case("zh"));
        if requires_chinese {
            for text in candidate_text_fields(&shot.plan) {
                let detected = language_detector.detect_language(text)?;
                if detected != "zh" {
                    reasons.push(format!(
                        "shot {} requires Chinese but language gate detected {detected}",
                        shot.id
                    ));
                }
            }
        }
    }

    if (total_duration - expected_duration_s).abs() > 0.001 {
        reasons.push(format!(
            "scene duration mismatch: expected {expected_duration_s}, got {total_duration}"
        ));
    }

    if reasons.is_empty() {
        Ok(L0Report::pass())
    } else {
        Ok(L0Report::repair(reasons))
    }
}

struct ShotForL0 {
    id: String,
    plan: Value,
    continuity_in: Option<String>,
    continuity_out: Option<String>,
}

fn is_boundary_reference(value: &str) -> bool {
    matches!(value, "start" | "end" | "none" | "null")
}

fn candidate_text_fields(plan: &Value) -> Vec<&str> {
    ["text", "subtitle", "voiceover", "caption", "title"]
        .iter()
        .filter_map(|key| plan.get(*key).and_then(Value::as_str))
        .filter(|text| !text.trim().is_empty())
        .collect()
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF
    )
}

fn parse_fasttext_label(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .find_map(|token| token.strip_prefix("__label__"))
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::test_support::temp_db_path;

    fn seeded_scene(name: &str) -> (std::path::PathBuf, Database) {
        let path = temp_db_path(name);
        let db = Database::open(&path).expect("open db");
        db.create_project("p", 100.0).unwrap();
        db.create_chapter("c", "p", "{}").unwrap();
        db.create_scene("s", "c", "{}").unwrap();
        (path, db)
    }

    #[test]
    fn l0_rejects_unclosed_references() {
        let (path, db) = seeded_scene("l0-ref");
        db.create_shot(
            "shot-1",
            "s",
            "{\"duration_s\":2,\"continuity_in\":\"missing-shot\"}",
            None,
            None,
            false,
            "standard",
        )
        .unwrap();

        let report =
            validate_scene_l0(db.connection(), "s", 2.0, &HeuristicLanguageDetector).unwrap();
        assert_eq!(report.verdict, L0Verdict::Repair);
        assert!(report
            .reasons
            .iter()
            .any(|reason| reason.contains("unclosed")));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn l0_repairs_english_text_when_chinese_required() {
        let (path, db) = seeded_scene("l0-lang");
        db.create_shot(
            "shot-1",
            "s",
            "{\"duration_s\":2,\"requires_chinese\":true,\"subtitle\":\"hello world\"}",
            None,
            None,
            false,
            "standard",
        )
        .unwrap();

        let report =
            validate_scene_l0(db.connection(), "s", 2.0, &HeuristicLanguageDetector).unwrap();
        assert_eq!(report.verdict, L0Verdict::Repair);
        assert!(report
            .reasons
            .iter()
            .any(|reason| reason.contains("detected en")));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn l0_passes_valid_chinese_scene_and_duration_budget() {
        let (path, db) = seeded_scene("l0-pass");
        db.create_shot(
            "shot-1",
            "s",
            "{\"duration_s\":1.5,\"requires_chinese\":true,\"subtitle\":\"这是中文\",\"continuity_out\":\"shot-2\"}",
            Some("start"),
            Some("shot-2"),
            true,
            "hero",
        )
        .unwrap();
        db.create_shot(
            "shot-2",
            "s",
            "{\"duration_s\":2.5,\"continuity_in\":\"shot-1\"}",
            Some("shot-1"),
            Some("end"),
            false,
            "standard",
        )
        .unwrap();

        let report =
            validate_scene_l0(db.connection(), "s", 4.0, &HeuristicLanguageDetector).unwrap();
        assert_eq!(report, L0Report::pass());

        let hero: i64 = db
            .connection()
            .query_row("SELECT is_hero FROM shots WHERE id='shot-1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let tier: String = db
            .connection()
            .query_row("SELECT tier FROM shots WHERE id='shot-1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(hero, 1);
        assert_eq!(tier, "hero");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn parses_fasttext_language_labels() {
        assert_eq!(
            parse_fasttext_label("__label__zh 0.998\n").as_deref(),
            Some("zh")
        );
        assert_eq!(
            parse_fasttext_label("  __label__en\n").as_deref(),
            Some("en")
        );
        assert_eq!(parse_fasttext_label("not-a-label"), None);
    }
}
