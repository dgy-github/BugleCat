use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

use crate::{Result, VideoAgentError};

#[derive(Debug, Clone, PartialEq)]
pub struct MediaProbe {
    pub path: String,
    pub duration_s: Option<f64>,
    pub video_streams: usize,
    pub audio_streams: usize,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub frame_rate: Option<f64>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaL0Report {
    pub passed: bool,
    pub reasons: Vec<String>,
    pub probe: Option<MediaProbe>,
}

impl MediaL0Report {
    pub fn layers_json(&self) -> Value {
        json!({
            "passed": self.passed,
            "reasons": self.reasons,
            "probe": self.probe.as_ref().map(media_probe_json),
        })
    }
}

pub fn validate_video_file_l0(
    path: impl AsRef<Path>,
    expected_duration_s: Option<f64>,
    tolerance_s: f64,
    require_audio: bool,
) -> Result<MediaL0Report> {
    let path = path.as_ref();
    if !path.is_file() {
        return Ok(repair(
            vec![format!("media file does not exist: {}", path.display())],
            None,
        ));
    }
    let size_bytes = path
        .metadata()
        .map_err(|err| {
            VideoAgentError::Ffmpeg(format!(
                "read media metadata {} failed: {err}",
                path.display()
            ))
        })?
        .len();
    if size_bytes == 0 {
        return Ok(repair(
            vec![format!("media file is empty: {}", path.display())],
            None,
        ));
    }

    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .map_err(|err| VideoAgentError::Ffmpeg(format!("failed to launch ffprobe: {err}")))?;
    if !output.status.success() {
        return Ok(repair(
            vec![format!(
                "ffprobe could not parse {}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )],
            None,
        ));
    }

    let raw: Value = serde_json::from_slice(&output.stdout).map_err(|err| {
        VideoAgentError::Ffmpeg(format!(
            "ffprobe returned non-JSON for {}: {err}",
            path.display()
        ))
    })?;
    let probe = media_probe_from_ffprobe(path, size_bytes, &raw);
    let mut reasons = Vec::new();
    if probe.video_streams == 0 {
        reasons.push("media has no video stream".to_string());
    }
    if probe.duration_s.unwrap_or_default() <= 0.0 {
        reasons.push("media duration is missing or non-positive".to_string());
    }
    if probe.width.unwrap_or_default() <= 0 || probe.height.unwrap_or_default() <= 0 {
        reasons.push("video dimensions are missing or invalid".to_string());
    }
    if require_audio && probe.audio_streams == 0 {
        reasons.push("media has no audio stream".to_string());
    }
    if let (Some(expected), Some(actual)) = (expected_duration_s, probe.duration_s) {
        if (actual - expected).abs() > tolerance_s.max(0.0) {
            reasons.push(format!(
                "media duration mismatch: expected {expected}, got {actual}"
            ));
        }
    }

    if reasons.is_empty() {
        Ok(MediaL0Report {
            passed: true,
            reasons,
            probe: Some(probe),
        })
    } else {
        Ok(repair(reasons, Some(probe)))
    }
}

fn media_probe_from_ffprobe(path: &Path, size_bytes: u64, raw: &Value) -> MediaProbe {
    let streams = raw
        .get("streams")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let video_streams = streams
        .iter()
        .filter(|stream| stream.get("codec_type").and_then(Value::as_str) == Some("video"))
        .count();
    let audio_streams = streams
        .iter()
        .filter(|stream| stream.get("codec_type").and_then(Value::as_str) == Some("audio"))
        .count();
    let first_video = streams
        .iter()
        .find(|stream| stream.get("codec_type").and_then(Value::as_str) == Some("video"));
    let duration_s = raw
        .get("format")
        .and_then(|format| format.get("duration"))
        .and_then(parse_json_f64)
        .or_else(|| {
            first_video
                .and_then(|stream| stream.get("duration"))
                .and_then(parse_json_f64)
        });

    MediaProbe {
        path: path.to_string_lossy().to_string(),
        duration_s,
        video_streams,
        audio_streams,
        width: first_video
            .and_then(|stream| stream.get("width"))
            .and_then(Value::as_i64),
        height: first_video
            .and_then(|stream| stream.get("height"))
            .and_then(Value::as_i64),
        frame_rate: first_video
            .and_then(|stream| {
                stream
                    .get("avg_frame_rate")
                    .or_else(|| stream.get("r_frame_rate"))
            })
            .and_then(Value::as_str)
            .and_then(parse_rate),
        size_bytes,
    }
}

fn repair(reasons: Vec<String>, probe: Option<MediaProbe>) -> MediaL0Report {
    MediaL0Report {
        passed: false,
        reasons,
        probe,
    }
}

fn media_probe_json(probe: &MediaProbe) -> Value {
    json!({
        "path": probe.path,
        "duration_s": probe.duration_s,
        "video_streams": probe.video_streams,
        "audio_streams": probe.audio_streams,
        "width": probe.width,
        "height": probe.height,
        "frame_rate": probe.frame_rate,
        "size_bytes": probe.size_bytes,
    })
}

fn parse_json_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
}

fn parse_rate(rate: &str) -> Option<f64> {
    let (num, den) = rate.split_once('/')?;
    let num = num.parse::<f64>().ok()?;
    let den = den.parse::<f64>().ok()?;
    (den != 0.0).then_some(num / den)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::test_support::temp_db_path;

    #[test]
    fn media_l0_passes_parseable_video_with_expected_duration() {
        if Command::new("ffmpeg").arg("-version").output().is_err()
            || Command::new("ffprobe").arg("-version").output().is_err()
        {
            eprintln!("ffmpeg/ffprobe not available; skipping media L0 smoke");
            return;
        }

        let path = temp_db_path("media-l0").with_extension("mp4");
        make_test_clip(&path, false);
        let report = validate_video_file_l0(&path, Some(0.3), 0.15, false).unwrap();

        assert!(report.passed, "{:?}", report.reasons);
        let probe = report.probe.unwrap();
        assert_eq!(probe.video_streams, 1);
        assert_eq!(probe.audio_streams, 0);
        assert_eq!(probe.width, Some(160));
        assert_eq!(probe.height, Some(90));
        assert!(probe.duration_s.unwrap() > 0.0);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn media_l0_rejects_missing_audio_when_required() {
        if Command::new("ffmpeg").arg("-version").output().is_err()
            || Command::new("ffprobe").arg("-version").output().is_err()
        {
            eprintln!("ffmpeg/ffprobe not available; skipping media L0 smoke");
            return;
        }

        let path = temp_db_path("media-audio-required").with_extension("mp4");
        make_test_clip(&path, false);
        let report = validate_video_file_l0(&path, Some(0.3), 0.15, true).unwrap();
        assert!(!report.passed);
        assert!(report
            .reasons
            .iter()
            .any(|reason| reason.contains("no audio")));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn media_l0_reports_missing_file_as_repair() {
        let path = temp_db_path("media-missing").with_extension("mp4");
        let report = validate_video_file_l0(&path, Some(1.0), 0.1, false).unwrap();
        assert!(!report.passed);
        assert!(report.probe.is_none());
        assert!(report.reasons[0].contains("does not exist"));
    }

    fn make_test_clip(path: &Path, with_audio: bool) {
        let mut command = Command::new("ffmpeg");
        command.args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=purple:s=160x90:d=0.3:r=10",
        ]);
        if with_audio {
            command.args([
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=0.3",
                "-shortest",
                "-c:a",
                "aac",
            ]);
        } else {
            command.arg("-an");
        }
        let status = command
            .args(["-pix_fmt", "yuv420p"])
            .arg(path)
            .status()
            .expect("run ffmpeg");
        assert!(status.success(), "test clip generation failed");
    }
}
