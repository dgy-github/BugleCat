use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use crate::{Result, VideoAgentError};

#[derive(Debug, Clone)]
pub struct RenderedShot {
    pub shot_id: String,
    pub clip_path: Option<PathBuf>,
    pub subtitle_path: Option<PathBuf>,
    pub audio_path: Option<PathBuf>,
    pub rerun_context: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FailedShot {
    pub shot_id: String,
    pub reason: String,
    pub rerun_context: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoughCutResult {
    pub rough_cut_path: Option<PathBuf>,
    pub failed_shots_path: PathBuf,
    pub assembly_manifest_path: PathBuf,
    pub failed: Vec<FailedShot>,
}

struct AssemblyClip {
    shot_id: String,
    path: PathBuf,
    subtitle_burned_in: bool,
    audio_muxed: bool,
    silent_audio_inserted: bool,
    notes: Vec<String>,
}

pub fn build_rough_cut(
    shots: &[RenderedShot],
    out_dir: impl AsRef<Path>,
) -> Result<RoughCutResult> {
    let out_dir = out_dir.as_ref();
    std::fs::create_dir_all(out_dir)
        .map_err(|err| VideoAgentError::Ffmpeg(format!("create output dir failed: {err}")))?;

    let mut valid = Vec::new();
    let mut failed = Vec::new();
    for shot in shots {
        match shot.clip_path.as_deref() {
            Some(path) if path.is_file() && path.metadata().map(|m| m.len()).unwrap_or(0) > 0 => {
                valid.push(shot);
            }
            Some(path) => failed.push(FailedShot {
                shot_id: shot.shot_id.clone(),
                reason: format!("clip missing or empty: {}", path.display()),
                rerun_context: shot.rerun_context.clone(),
            }),
            None => failed.push(FailedShot {
                shot_id: shot.shot_id.clone(),
                reason: "clip not rendered".to_string(),
                rerun_context: shot.rerun_context.clone(),
            }),
        }
    }

    let needs_post = valid.iter().any(|shot| {
        usable_file(shot.subtitle_path.as_deref()) || usable_file(shot.audio_path.as_deref())
    });
    let needs_audio_track = valid
        .iter()
        .any(|shot| usable_file(shot.audio_path.as_deref()));

    let mut assembled = Vec::new();
    for shot in &valid {
        assembled.push(assemble_shot_clip(
            shot,
            out_dir,
            needs_post,
            needs_audio_track,
        )?);
    }

    let manifest_rows = shots
        .iter()
        .map(|shot| {
            let assembly = assembled.iter().find(|clip| clip.shot_id == shot.shot_id);
            let missing_media = missing_optional_media_notes(shot);
            json!({
                "shot_id": shot.shot_id,
                "clip_path": shot.clip_path.as_ref().map(|path| path.to_string_lossy().to_string()),
                "subtitle_path": shot.subtitle_path.as_ref().map(|path| path.to_string_lossy().to_string()),
                "audio_path": shot.audio_path.as_ref().map(|path| path.to_string_lossy().to_string()),
                "assembled_clip_path": assembly.map(|clip| clip.path.to_string_lossy().to_string()),
                "subtitle_burned_in": assembly.map(|clip| clip.subtitle_burned_in).unwrap_or(false),
                "audio_muxed": assembly.map(|clip| clip.audio_muxed).unwrap_or(false),
                "silent_audio_inserted": assembly.map(|clip| clip.silent_audio_inserted).unwrap_or(false),
                "assembly_notes": assembly.map(|clip| clip.notes.clone()).unwrap_or_else(|| missing_media),
                "rerun_context": shot.rerun_context,
            })
        })
        .collect::<Vec<_>>();

    let failed_shots_path = out_dir.join("failed_shots.json");
    write_failed_shots(&failed_shots_path, &failed)?;
    let assembly_manifest_path = out_dir.join("assembly_manifest.json");
    write_assembly_manifest(&assembly_manifest_path, &manifest_rows)?;

    let rough_cut_path = if assembled.is_empty() {
        None
    } else {
        let rough_cut = out_dir.join("rough_cut.mp4");
        let clips = assembled
            .iter()
            .map(|clip| (clip.shot_id.clone(), clip.path.clone()))
            .collect::<Vec<_>>();
        concat_clips(&clips, &rough_cut, out_dir)?;
        Some(rough_cut)
    };

    Ok(RoughCutResult {
        rough_cut_path,
        failed_shots_path,
        assembly_manifest_path,
        failed,
    })
}

fn write_failed_shots(path: &Path, failed: &[FailedShot]) -> Result<()> {
    let rows = failed
        .iter()
        .map(|shot| {
            json!({
                "shot_id": shot.shot_id,
                "reason": shot.reason,
                "rerun_context": shot.rerun_context,
            })
        })
        .collect::<Vec<_>>();
    let text = serde_json::to_string_pretty(&rows)?;
    std::fs::write(path, text)
        .map_err(|err| VideoAgentError::Ffmpeg(format!("write failed_shots.json failed: {err}")))?;
    Ok(())
}

fn write_assembly_manifest(path: &Path, rows: &[Value]) -> Result<()> {
    let text = serde_json::to_string_pretty(&json!({
        "note": "Video clips are normalized with FFmpeg before concat. SRT subtitles are burned in when present; provided audio is muxed, and silence is inserted only to keep clip stream layouts compatible. missing audio is not treated as synthesized TTS.",
        "shots": rows,
    }))?;
    std::fs::write(path, text).map_err(|err| {
        VideoAgentError::Ffmpeg(format!("write assembly_manifest.json failed: {err}"))
    })?;
    Ok(())
}

fn assemble_shot_clip(
    shot: &RenderedShot,
    out_dir: &Path,
    normalize: bool,
    needs_audio_track: bool,
) -> Result<AssemblyClip> {
    let clip_path = shot
        .clip_path
        .as_ref()
        .expect("assemble_shot_clip only receives valid clips");
    let subtitle_path = usable_file(shot.subtitle_path.as_deref()).then(|| {
        shot.subtitle_path
            .as_ref()
            .expect("usable subtitle path exists")
            .to_path_buf()
    });
    let audio_path = usable_file(shot.audio_path.as_deref()).then(|| {
        shot.audio_path
            .as_ref()
            .expect("usable audio path exists")
            .to_path_buf()
    });
    let mut notes = missing_optional_media_notes(shot);
    let subtitle_burned_in = subtitle_path.is_some();
    let audio_muxed = audio_path.is_some();
    let silent_audio_inserted = needs_audio_track && audio_path.is_none();

    if !normalize {
        return Ok(AssemblyClip {
            shot_id: shot.shot_id.clone(),
            path: clip_path.clone(),
            subtitle_burned_in,
            audio_muxed,
            silent_audio_inserted,
            notes,
        });
    }

    let work_dir = out_dir.join("_rough_cut_work");
    std::fs::create_dir_all(&work_dir).map_err(|err| {
        VideoAgentError::Ffmpeg(format!("create rough-cut work dir failed: {err}"))
    })?;
    let assembled_path = work_dir.join(format!("{}.mp4", sanitize_filename(&shot.shot_id)));

    let mut command = Command::new("ffmpeg");
    command.args(["-y", "-v", "error"]).arg("-i").arg(clip_path);
    if let Some(audio_path) = &audio_path {
        command.arg("-i").arg(audio_path);
    } else if needs_audio_track {
        command
            .args(["-f", "lavfi"])
            .args(["-i", "anullsrc=channel_layout=stereo:sample_rate=48000"]);
    }

    if let Some(subtitle_path) = &subtitle_path {
        command.args(["-vf", &subtitle_filter(subtitle_path)]);
    }

    command.args(["-map", "0:v:0"]);
    if needs_audio_track {
        command.args(["-map", "1:a:0", "-shortest"]);
    } else {
        command.arg("-an");
    }
    command.args([
        "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p",
    ]);
    if needs_audio_track {
        command.args(["-c:a", "aac", "-b:a", "128k", "-ar", "48000", "-ac", "2"]);
    }
    command.arg(&assembled_path);

    let output = command
        .output()
        .map_err(|err| VideoAgentError::Ffmpeg(format!("failed to launch ffmpeg: {err}")))?;
    if !output.status.success() {
        return Err(VideoAgentError::Ffmpeg(format!(
            "ffmpeg shot assembly failed for {}: {}",
            shot.shot_id,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    if subtitle_burned_in {
        notes.push("subtitle burned into video".to_string());
    }
    if audio_muxed {
        notes.push("audio muxed into clip".to_string());
    } else if silent_audio_inserted {
        notes.push("silent audio inserted to keep concat streams compatible".to_string());
    }

    Ok(AssemblyClip {
        shot_id: shot.shot_id.clone(),
        path: assembled_path,
        subtitle_burned_in,
        audio_muxed,
        silent_audio_inserted,
        notes,
    })
}

fn concat_clips(clips: &[(String, PathBuf)], dest: &Path, out_dir: &Path) -> Result<()> {
    if clips.len() == 1 {
        std::fs::copy(&clips[0].1, dest).map_err(|err| {
            VideoAgentError::Ffmpeg(format!(
                "copy single clip {} to {} failed: {err}",
                clips[0].1.display(),
                dest.display()
            ))
        })?;
        return Ok(());
    }

    let list_path = out_dir.join("_rough_cut_concat.txt");
    let listing = clips
        .iter()
        .map(|(_, path)| format!("file '{}'\n", ffmpeg_concat_path(path)))
        .collect::<String>();
    std::fs::write(&list_path, listing)
        .map_err(|err| VideoAgentError::Ffmpeg(format!("write concat list failed: {err}")))?;

    let output = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list_path)
        .args(["-c", "copy"])
        .arg(dest)
        .output()
        .map_err(|err| VideoAgentError::Ffmpeg(format!("failed to launch ffmpeg: {err}")))?;

    let _ = std::fs::remove_file(&list_path);
    if output.status.success() {
        Ok(())
    } else {
        Err(VideoAgentError::Ffmpeg(format!(
            "ffmpeg concat failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

fn ffmpeg_concat_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .replace('\'', "'\\''")
}

fn subtitle_filter(path: &Path) -> String {
    format!("subtitles='{}'", ffmpeg_filter_path(path))
}

fn ffmpeg_filter_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .replace(':', "\\:")
        .replace('\'', "\\'")
}

fn usable_file(path: Option<&Path>) -> bool {
    path.is_some_and(|path| path.is_file() && path.metadata().map(|m| m.len()).unwrap_or(0) > 0)
}

fn missing_optional_media_notes(shot: &RenderedShot) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(path) = &shot.subtitle_path {
        if !usable_file(Some(path)) {
            notes.push(format!("subtitle missing or empty: {}", path.display()));
        }
    }
    if let Some(path) = &shot.audio_path {
        if !usable_file(Some(path)) {
            notes.push(format!("audio missing or empty: {}", path.display()));
        }
    }
    notes
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
    use std::process::Command;

    use serde_json::json;

    use super::*;
    use crate::test_support::temp_db_path;
    use crate::text_separation::{write_srt, OverlayText};

    #[test]
    fn partial_delivery_writes_failed_shots_even_without_any_clips() {
        let out_dir = temp_db_path("rough-no-clips");
        std::fs::create_dir_all(&out_dir).unwrap();
        let result = build_rough_cut(
            &[RenderedShot {
                shot_id: "shot-1".to_string(),
                clip_path: None,
                subtitle_path: None,
                audio_path: None,
                rerun_context: json!({"prompt": "retry me"}),
            }],
            &out_dir,
        )
        .unwrap();

        assert!(result.rough_cut_path.is_none());
        assert_eq!(result.failed.len(), 1);
        let failed_json: Value =
            serde_json::from_str(&std::fs::read_to_string(&result.failed_shots_path).unwrap())
                .unwrap();
        assert_eq!(failed_json[0]["shot_id"], "shot-1");
        assert_eq!(failed_json[0]["rerun_context"]["prompt"], "retry me");
        let manifest_json: Value =
            serde_json::from_str(&std::fs::read_to_string(&result.assembly_manifest_path).unwrap())
                .unwrap();
        assert_eq!(manifest_json["shots"][0]["shot_id"], "shot-1");

        let _ = std::fs::remove_dir_all(out_dir);
    }

    #[test]
    fn ffmpeg_builds_rough_cut_and_keeps_failed_context() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            eprintln!("ffmpeg not available; skipping rough_cut smoke");
            return;
        }

        let out_dir = temp_db_path("rough-cut");
        std::fs::create_dir_all(&out_dir).unwrap();
        let clip_a = out_dir.join("a.mp4");
        let clip_b = out_dir.join("b.mp4");
        let srt_a = out_dir.join("shot-1.srt");
        let wav_b = out_dir.join("shot-3.wav");
        make_test_clip(&clip_a, "red");
        make_test_clip(&clip_b, "blue");
        write_srt(
            &srt_a,
            &[OverlayText {
                text: "HELLO".to_string(),
                start_s: 0.0,
                end_s: 0.18,
            }],
        )
        .unwrap();
        make_test_audio(&wav_b);

        let result = build_rough_cut(
            &[
                RenderedShot {
                    shot_id: "shot-1".to_string(),
                    clip_path: Some(clip_a),
                    subtitle_path: Some(srt_a),
                    audio_path: None,
                    rerun_context: json!({"attempt": 0}),
                },
                RenderedShot {
                    shot_id: "shot-2".to_string(),
                    clip_path: None,
                    subtitle_path: None,
                    audio_path: None,
                    rerun_context: json!({"attempt": 1}),
                },
                RenderedShot {
                    shot_id: "shot-3".to_string(),
                    clip_path: Some(clip_b),
                    subtitle_path: None,
                    audio_path: Some(wav_b),
                    rerun_context: json!({"attempt": 0}),
                },
            ],
            &out_dir,
        )
        .unwrap();

        let rough = result.rough_cut_path.expect("rough cut path");
        assert!(rough.is_file());
        assert!(rough.metadata().unwrap().len() > 0);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].shot_id, "shot-2");
        let manifest: Value =
            serde_json::from_str(&std::fs::read_to_string(&result.assembly_manifest_path).unwrap())
                .unwrap();
        assert!(manifest["note"].as_str().unwrap().contains("missing audio"));
        assert_eq!(manifest["shots"][0]["subtitle_burned_in"], true);
        assert_eq!(manifest["shots"][0]["silent_audio_inserted"], true);
        assert_eq!(manifest["shots"][2]["audio_muxed"], true);
        assert!(manifest["shots"][0]["assembled_clip_path"]
            .as_str()
            .is_some());
        assert_eq!(ffprobe_audio_stream_count(&rough), 1);

        let _ = std::fs::remove_dir_all(out_dir);
    }

    fn make_test_clip(path: &Path, color: &str) {
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                &format!("color=c={color}:s=160x90:d=0.2:r=10"),
                "-pix_fmt",
                "yuv420p",
                "-an",
            ])
            .arg(path)
            .status()
            .expect("run ffmpeg to make test clip");
        assert!(status.success(), "test clip generation failed");
    }

    fn make_test_audio(path: &Path) {
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=0.2",
                "-ac",
                "2",
                "-ar",
                "48000",
            ])
            .arg(path)
            .status()
            .expect("run ffmpeg to make test audio");
        assert!(status.success(), "test audio generation failed");
    }

    fn ffprobe_audio_stream_count(path: &Path) -> usize {
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "a",
                "-show_entries",
                "stream=index",
                "-of",
                "csv=p=0",
            ])
            .arg(path)
            .output()
            .expect("run ffprobe");
        assert!(output.status.success(), "ffprobe failed");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
    }
}
