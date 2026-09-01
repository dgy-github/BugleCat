use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use ncx_video_agent::{
    Database, FastTextModelDetector, LanguageDetector, P1ExternalConfig, ReqwestTosTransport,
    ResolvedSetting, TosClient, TosConfig,
};

fn main() -> ExitCode {
    let external_config = P1ExternalConfig::load();
    let checks = vec![
        check_sqlite(),
        check_temporal_port(),
        check_ffmpeg(),
        check_opencv(),
        check_fasttext(),
        check_resolved_setting(
            "ARK_API_KEY",
            external_config.ark_api_key.as_ref(),
            &[
                "ARK_API_KEY",
                "NANOCODEX_ARK_API_KEY",
                "ncx-config ark_api_key",
            ],
        ),
        check_tos_roundtrip(),
        check_resolved_setting(
            "VL_API_KEY",
            external_config.vl_api_key.as_ref(),
            &[
                "VL_API_KEY",
                "NANOCODEX_VL_API_KEY",
                "ncx-config vl_api_key",
            ],
        ),
        check_resolved_setting(
            "VL_BASE_URL",
            external_config.vl_base_url.as_ref(),
            &[
                "VL_BASE_URL",
                "NANOCODEX_VL_BASE_URL",
                "ncx-config vl_base_url",
            ],
        ),
        check_resolved_setting(
            "VL_MODEL",
            external_config.vl_model.as_ref(),
            &["VL_MODEL", "NANOCODEX_VL_MODEL", "ncx-config vl_model"],
        ),
        check_config_load(external_config.config_error.as_deref()),
    ];

    let width = checks
        .iter()
        .map(|check| check.name.len())
        .max()
        .unwrap_or(0);
    for check in &checks {
        let status = if check.ok { "PASS" } else { "FAIL" };
        println!(
            "{status}  {name:width$}  {detail}",
            name = check.name,
            detail = check.detail,
        );
    }

    if checks.iter().all(|check| check.ok) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
}

fn check_sqlite() -> Check {
    let path = std::env::temp_dir().join(format!(
        "ncx-video-agent-smoke-{}.sqlite",
        std::process::id()
    ));
    let result = (|| {
        let db = Database::open(&path)?;
        db.create_project("smoke", 1.0)?;
        let mode: String = db
            .connection()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        let value: i64 =
            db.connection()
                .query_row("SELECT json_extract('{\"ok\":1}', '$.ok')", [], |row| {
                    row.get(0)
                })?;
        Ok::<_, ncx_video_agent::VideoAgentError>((mode, value))
    })();
    let _ = std::fs::remove_file(&path);

    match result {
        Ok((mode, 1)) if mode.eq_ignore_ascii_case("wal") => Check {
            name: "SQLite WAL + JSON1",
            ok: true,
            detail: format!("journal_mode={mode}"),
        },
        Ok((mode, value)) => Check {
            name: "SQLite WAL + JSON1",
            ok: false,
            detail: format!("unexpected journal_mode={mode}, json_extract={value}"),
        },
        Err(err) => Check {
            name: "SQLite WAL + JSON1",
            ok: false,
            detail: err.to_string(),
        },
    }
}

fn check_temporal_port() -> Check {
    let raw = std::env::var("TEMPORAL_ADDRESS").unwrap_or_else(|_| "127.0.0.1:7233".to_string());
    let address = raw
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .to_string();

    let resolved = address
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next());
    let Some(socket_addr) = resolved else {
        return Check {
            name: "Temporal port",
            ok: false,
            detail: format!("could not resolve TEMPORAL_ADDRESS={raw}"),
        };
    };

    match TcpStream::connect_timeout(&socket_addr, Duration::from_secs(2)) {
        Ok(_) => Check {
            name: "Temporal port",
            ok: true,
            detail: format!("connected to {address}"),
        },
        Err(err) => Check {
            name: "Temporal port",
            ok: false,
            detail: format!("cannot connect to {address}: {err}"),
        },
    }
}

fn check_ffmpeg() -> Check {
    match Command::new("ffmpeg").arg("-version").output() {
        Ok(output) if output.status.success() => {
            let first = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("ffmpeg available")
                .to_string();
            Check {
                name: "FFmpeg",
                ok: true,
                detail: first,
            }
        }
        Ok(output) => Check {
            name: "FFmpeg",
            ok: false,
            detail: format!("ffmpeg exited with {}", output.status),
        },
        Err(err) => Check {
            name: "FFmpeg",
            ok: false,
            detail: err.to_string(),
        },
    }
}

fn check_opencv() -> Check {
    match Command::new("opencv_version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("opencv_version available")
                .trim()
                .to_string();
            return Check {
                name: "OpenCV",
                ok: true,
                detail: format!("opencv_version {version}"),
            };
        }
        _ => {}
    }

    let mut candidates = Vec::new();
    if let Ok(value) = std::env::var("PYTHON") {
        let value = value.trim();
        if !value.is_empty() {
            candidates.push(value.to_string());
        }
    }
    candidates.push("python".to_string());

    let script = r#"
import cv2
import numpy as np
img = np.zeros((4, 4, 3), dtype=np.uint8)
gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
print(f"cv2 {cv2.__version__} gray={gray.shape[0]}x{gray.shape[1]}")
"#;
    let mut errors = Vec::new();
    for candidate in candidates {
        match Command::new(&candidate).args(["-c", script]).output() {
            Ok(output) if output.status.success() => {
                let detail = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .unwrap_or("cv2 probe passed")
                    .trim()
                    .to_string();
                return Check {
                    name: "OpenCV",
                    ok: true,
                    detail: format!("{candidate}: {detail}"),
                };
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                errors.push(format!(
                    "{candidate} exited with {}: {}",
                    output.status,
                    stderr.trim()
                ));
            }
            Err(err) => errors.push(format!("{candidate}: {err}")),
        }
    }

    Check {
        name: "OpenCV",
        ok: false,
        detail: format!(
            "install opencv_version or Python cv2; probes failed: {}",
            errors.join(" | ")
        ),
    }
}

fn check_fasttext() -> Check {
    let Some((source, model)) =
        first_env_value(&["FASTTEXT_LID_MODEL", "LID_176_BIN", "LID_176_FTZ"])
    else {
        return Check {
            name: "fastText lid",
            ok: false,
            detail:
                "set FASTTEXT_LID_MODEL, LID_176_BIN, or LID_176_FTZ to an official lid.176 model"
                    .to_string(),
        };
    };
    if !Path::new(&model).is_file() {
        return Check {
            name: "fastText lid",
            ok: false,
            detail: format!("model file not found: {model}"),
        };
    }

    match FastTextModelDetector::load(&model)
        .and_then(|detector| detector.detect_language("这是中文"))
    {
        Ok(label) if label == "zh" => Check {
            name: "fastText lid",
            ok: true,
            detail: format!("pure-rust fastText {source} detected {label}"),
        },
        Ok(label) => Check {
            name: "fastText lid",
            ok: false,
            detail: format!("pure-rust fastText {source} detected unexpected {label}"),
        },
        Err(err) => {
            let cli = check_fasttext_cli(&model);
            if cli.ok {
                return cli;
            }
            Check {
                name: "fastText lid",
                ok: false,
                detail: format!("{err}; CLI fallback: {}", cli.detail),
            }
        }
    }
}

fn check_fasttext_cli(model: &str) -> Check {
    let child = Command::new("fasttext")
        .args(["predict", model, "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(err) => {
            return Check {
                name: "fastText lid",
                ok: false,
                detail: format!("fasttext CLI not available: {err}"),
            };
        }
    };
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all("这是中文\n".as_bytes());
    }
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(err) => {
            return Check {
                name: "fastText lid",
                ok: false,
                detail: format!("fasttext predict failed: {err}"),
            };
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let ok = output.status.success() && stdout.contains("__label__zh");
    Check {
        name: "fastText lid",
        ok,
        detail: if ok {
            stdout.trim().to_string()
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!(
                "unexpected output: {}; stderr: {}",
                stdout.trim(),
                stderr.trim()
            )
        },
    }
}

fn check_tos_roundtrip() -> Check {
    let config = match TosConfig::from_env() {
        Ok(config) => config,
        Err(err) => {
            return Check {
                name: "TOS roundtrip",
                ok: false,
                detail: err.to_string(),
            };
        }
    };
    let transport = match ReqwestTosTransport::new() {
        Ok(transport) => transport,
        Err(err) => {
            return Check {
                name: "TOS roundtrip",
                ok: false,
                detail: err.to_string(),
            };
        }
    };
    let mut client = TosClient::new(config, transport);
    let key = format!(
        "ncx-video-agent/p1-smoke/{}-{}.txt",
        std::process::id(),
        chrono_free_timestamp()
    );
    let body = format!("ncx-video-agent p1 smoke {}\n", std::process::id()).into_bytes();

    let put = match client.put_object(&key, &body, "text/plain; charset=utf-8") {
        Ok(object) => object,
        Err(err) => {
            return Check {
                name: "TOS roundtrip",
                ok: false,
                detail: format!("upload failed: {err}"),
            };
        }
    };
    let downloaded = match client.get_object(&key) {
        Ok(downloaded) => downloaded,
        Err(err) => {
            return Check {
                name: "TOS roundtrip",
                ok: false,
                detail: format!("download failed after {}: {err}", put.uri),
            };
        }
    };
    if downloaded != body {
        return Check {
            name: "TOS roundtrip",
            ok: false,
            detail: format!("downloaded bytes differ for {}", put.uri),
        };
    }
    let cleanup = client.delete_object(&key);
    Check {
        name: "TOS roundtrip",
        ok: true,
        detail: match cleanup {
            Ok(()) => format!("put/get/delete {}", put.uri),
            Err(err) => format!("put/get {}; cleanup failed: {err}", put.uri),
        },
    }
}

fn chrono_free_timestamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn check_resolved_setting(
    name: &'static str,
    setting: Option<&ResolvedSetting>,
    expected_sources: &[&str],
) -> Check {
    if let Some(setting) = setting {
        Check {
            name,
            ok: true,
            detail: format!("set via {}", setting.source),
        }
    } else {
        Check {
            name,
            ok: false,
            detail: format!("missing one of: {}", expected_sources.join(", ")),
        }
    }
}

fn check_config_load(error: Option<&str>) -> Check {
    match error {
        None => Check {
            name: "ncx-config",
            ok: true,
            detail: "loaded main config for ARK/VL resolution".to_string(),
        },
        Some(error) => Check {
            name: "ncx-config",
            ok: false,
            detail: error.to_string(),
        },
    }
}

fn first_env_value(keys: &[&str]) -> Option<(String, String)> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(|value| ((*key).to_string(), value))
    })
}
