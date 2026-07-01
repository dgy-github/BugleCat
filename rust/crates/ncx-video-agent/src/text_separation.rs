use std::path::{Path, PathBuf};

use crate::{Result, VideoAgentError};

#[derive(Debug, Clone, PartialEq)]
pub struct OverlayText {
    pub text: String,
    pub start_s: f64,
    pub end_s: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShotTextSpec {
    pub shot_id: String,
    pub visual_prompt: String,
    pub duration_s: f64,
    pub overlays: Vec<OverlayText>,
    pub dialogue: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TtsRequest {
    pub shot_id: String,
    pub text: String,
    pub start_s: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeparatedShot {
    pub shot_id: String,
    pub clean_generation_prompt: String,
    pub srt_path: Option<PathBuf>,
    pub tts_requests: Vec<TtsRequest>,
    pub requires_post_text: bool,
}

pub fn separate_text_and_voice(
    spec: &ShotTextSpec,
    out_dir: impl AsRef<Path>,
) -> Result<SeparatedShot> {
    if spec.duration_s <= 0.0 {
        return Err(VideoAgentError::NodeContract(format!(
            "shot {} has non-positive duration",
            spec.shot_id
        )));
    }

    let clean_generation_prompt = enforce_no_text_overlays(&spec.visual_prompt);
    let out_dir = out_dir.as_ref();
    std::fs::create_dir_all(out_dir).map_err(|err| {
        VideoAgentError::Ffmpeg(format!("create text separation dir failed: {err}"))
    })?;

    let normalized = normalized_overlays(spec)?;
    let srt_path = if normalized.is_empty() {
        None
    } else {
        let path = out_dir.join(format!("{}.srt", sanitize_filename(&spec.shot_id)));
        write_srt(&path, &normalized)?;
        Some(path)
    };

    let tts_requests = spec
        .dialogue
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then(|| TtsRequest {
                shot_id: spec.shot_id.clone(),
                text: trimmed.to_string(),
                start_s: 0.0,
            })
        })
        .collect::<Vec<_>>();

    Ok(SeparatedShot {
        shot_id: spec.shot_id.clone(),
        clean_generation_prompt,
        srt_path,
        tts_requests,
        requires_post_text: !normalized.is_empty(),
    })
}

pub fn write_srt(path: impl AsRef<Path>, overlays: &[OverlayText]) -> Result<()> {
    let mut text = String::new();
    for (idx, overlay) in overlays.iter().enumerate() {
        text.push_str(&(idx + 1).to_string());
        text.push('\n');
        text.push_str(&format!(
            "{} --> {}\n",
            srt_timestamp(overlay.start_s),
            srt_timestamp(overlay.end_s)
        ));
        text.push_str(&overlay.text);
        text.push_str("\n\n");
    }
    std::fs::write(path.as_ref(), text)
        .map_err(|err| VideoAgentError::Ffmpeg(format!("write SRT failed: {err}")))?;
    Ok(())
}

fn enforce_no_text_overlays(prompt: &str) -> String {
    let trimmed = prompt.trim();
    if trimmed.to_ascii_lowercase().contains("no text overlays") {
        trimmed.to_string()
    } else if trimmed.is_empty() {
        "no text overlays".to_string()
    } else {
        format!("{trimmed}\n\nno text overlays")
    }
}

fn normalized_overlays(spec: &ShotTextSpec) -> Result<Vec<OverlayText>> {
    let mut overlays = Vec::new();
    for overlay in &spec.overlays {
        let start = overlay.start_s.max(0.0);
        let end = overlay.end_s.min(spec.duration_s);
        if overlay.text.trim().is_empty() {
            continue;
        }
        if end <= start {
            return Err(VideoAgentError::NodeContract(format!(
                "shot {} overlay has invalid time range {}..{}",
                spec.shot_id, overlay.start_s, overlay.end_s
            )));
        }
        overlays.push(OverlayText {
            text: overlay.text.trim().to_string(),
            start_s: start,
            end_s: end,
        });
    }
    overlays.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
    Ok(overlays)
}

fn srt_timestamp(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let total_m = total_s / 60;
    let m = total_m % 60;
    let h = total_m / 60;
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

fn sanitize_filename(value: &str) -> String {
    let clean = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if clean.trim_matches('_').is_empty() {
        "shot".to_string()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::test_support::temp_db_path;

    #[test]
    fn text_and_voice_are_split_from_generation_prompt() {
        let out_dir = temp_db_path("text-separation");
        let spec = ShotTextSpec {
            shot_id: "shot:01".to_string(),
            visual_prompt: "A storefront at night".to_string(),
            duration_s: 4.0,
            overlays: vec![OverlayText {
                text: "欢迎光临".to_string(),
                start_s: 0.5,
                end_s: 3.0,
            }],
            dialogue: vec!["旁白：欢迎来到这里".to_string()],
        };

        let separated = separate_text_and_voice(&spec, &out_dir).unwrap();
        assert!(separated
            .clean_generation_prompt
            .contains("no text overlays"));
        assert!(!separated.clean_generation_prompt.contains("欢迎光临"));
        assert_eq!(separated.tts_requests[0].text, "旁白：欢迎来到这里");

        let srt = std::fs::read_to_string(separated.srt_path.unwrap()).unwrap();
        assert!(srt.contains("00:00:00,500 --> 00:00:03,000"));
        assert!(srt.contains("欢迎光临"));

        let _ = std::fs::remove_dir_all(out_dir);
    }

    #[test]
    fn no_text_shot_has_no_srt_but_still_blocks_text_overlays() {
        let out_dir = temp_db_path("text-empty");
        let spec = ShotTextSpec {
            shot_id: "shot-02".to_string(),
            visual_prompt: "clean landscape, no text overlays".to_string(),
            duration_s: 2.0,
            overlays: vec![],
            dialogue: vec![],
        };
        let separated = separate_text_and_voice(&spec, &out_dir).unwrap();
        assert_eq!(
            separated.clean_generation_prompt,
            "clean landscape, no text overlays"
        );
        assert!(separated.srt_path.is_none());
        assert!(!separated.requires_post_text);
        let _ = std::fs::remove_dir_all(out_dir);
    }

    #[test]
    fn invalid_overlay_time_is_rejected() {
        let out_dir = temp_db_path("text-invalid");
        let spec = ShotTextSpec {
            shot_id: "shot-03".to_string(),
            visual_prompt: String::new(),
            duration_s: 2.0,
            overlays: vec![OverlayText {
                text: "bad".to_string(),
                start_s: 1.5,
                end_s: 1.0,
            }],
            dialogue: vec![],
        };
        let err =
            separate_text_and_voice(&spec, &out_dir).expect_err("invalid overlay range must fail");
        assert!(err.to_string().contains("invalid time range"));
        let _ = std::fs::remove_dir_all(out_dir);
    }

    #[test]
    fn srt_timestamp_rounds_milliseconds() {
        assert_eq!(srt_timestamp(3661.2346), "01:01:01,235");
        assert_eq!(srt_timestamp(-1.0), "00:00:00,000");
        let _ = json!({"keeps": "serde_json used in this crate"});
    }
}
