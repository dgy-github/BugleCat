use sha2::{Digest, Sha256};

use crate::ark::{ArkClient, ArkTransport};
use crate::db::Database;
use crate::jobs::{
    fail_job_and_release_budget, mark_job_status, submit_job_once, JobSubmitOutcome,
};
use crate::tos::{TosClient, TosTransport};
use crate::{Result, VideoAgentError};

#[derive(Debug, Clone, PartialEq)]
pub struct SeedanceSubmitInput {
    pub project_id: String,
    pub shot_id: String,
    pub attempt: i64,
    pub model: String,
    pub payload: serde_json::Value,
    pub reserve_cost: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeedanceArtifactInput {
    pub artifact_id: String,
    pub shot_id: String,
    pub tos_key: String,
    pub ark_task_id: String,
    pub video_url: String,
    pub usage: serde_json::Value,
    pub params_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeedanceArtifactOutput {
    pub artifact_id: String,
    pub tos_uri: String,
    pub content_hash: String,
    pub size_bytes: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SeedancePollOutcome {
    Running {
        status: String,
    },
    Succeeded {
        video_url: String,
        usage: serde_json::Value,
    },
    Failed {
        status: String,
        reason: String,
    },
}

pub trait VideoDownloader {
    fn download(&mut self, url: &str) -> std::result::Result<Vec<u8>, String>;
}

pub struct ReqwestVideoDownloader {
    client: reqwest::blocking::Client,
}

impl ReqwestVideoDownloader {
    pub fn new() -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(|err| VideoAgentError::Ark(format!("build HTTP client failed: {err}")))?;
        Ok(Self { client })
    }
}

impl VideoDownloader for ReqwestVideoDownloader {
    fn download(&mut self, url: &str) -> std::result::Result<Vec<u8>, String> {
        let response = self
            .client
            .get(url)
            .send()
            .map_err(|err| format!("download request failed: {err}"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(format!(
                "download failed (HTTP {}): {}",
                status.as_u16(),
                body
            ));
        }
        response
            .bytes()
            .map(|bytes| bytes.to_vec())
            .map_err(|err| format!("read downloaded video failed: {err}"))
    }
}

pub fn submit_seedance_job_once<T: ArkTransport>(
    db: &mut Database,
    ark: &mut ArkClient<T>,
    input: &SeedanceSubmitInput,
) -> Result<JobSubmitOutcome> {
    submit_job_once(
        db.connection_mut(),
        &input.project_id,
        &input.shot_id,
        input.attempt,
        &input.payload,
        "ark",
        &input.model,
        input.reserve_cost,
        || ark.submit(&input.payload).map_err(|err| err.to_string()),
    )
}

pub fn poll_seedance_job_once<T: ArkTransport>(
    db: &mut Database,
    ark: &mut ArkClient<T>,
    project_id: &str,
    job_id: &str,
    task_id: &str,
) -> Result<SeedancePollOutcome> {
    let status = ark.poll_once(task_id)?;
    match status.status.as_str() {
        "succeeded" => {
            let Some(video_url) = status.video_url else {
                let reason = format!("Seedance task {task_id} succeeded without video_url");
                fail_job_and_release_budget(db.connection_mut(), project_id, job_id, &reason)?;
                return Ok(SeedancePollOutcome::Failed {
                    status: "succeeded".to_string(),
                    reason,
                });
            };
            mark_job_status(db.connection(), job_id, "provider_succeeded")?;
            Ok(SeedancePollOutcome::Succeeded {
                video_url,
                usage: status.usage,
            })
        }
        "failed" | "cancelled" => {
            let reason = format!("Seedance task {task_id} ended as {}", status.status);
            fail_job_and_release_budget(db.connection_mut(), project_id, job_id, &reason)?;
            Ok(SeedancePollOutcome::Failed {
                status: status.status,
                reason,
            })
        }
        "" => Err(VideoAgentError::Ark(format!(
            "Seedance task {task_id} returned empty status"
        ))),
        other => {
            mark_job_status(db.connection(), job_id, "provider_running")?;
            Ok(SeedancePollOutcome::Running {
                status: other.to_string(),
            })
        }
    }
}

pub fn persist_seedance_video_artifact<T, D>(
    db: &Database,
    tos: &mut TosClient<T>,
    downloader: &mut D,
    input: &SeedanceArtifactInput,
) -> Result<SeedanceArtifactOutput>
where
    T: TosTransport,
    D: VideoDownloader,
{
    if input.video_url.trim().is_empty() {
        return Err(VideoAgentError::Ark(
            "Seedance task succeeded without a video_url".to_string(),
        ));
    }
    let bytes = downloader
        .download(&input.video_url)
        .map_err(VideoAgentError::Ark)?;
    if bytes.is_empty() {
        return Err(VideoAgentError::Ark(
            "downloaded Seedance video is empty".to_string(),
        ));
    }
    let object = tos.put_object(&input.tos_key, &bytes, "video/mp4")?;
    let artifact_params = artifact_params_json(input);
    db.create_artifact(
        &input.artifact_id,
        Some(&input.shot_id),
        "video",
        &object.uri,
        &object.content_hash,
        &artifact_params.to_string(),
    )?;
    Ok(SeedanceArtifactOutput {
        artifact_id: input.artifact_id.clone(),
        tos_uri: object.uri,
        content_hash: object.content_hash,
        size_bytes: object.size_bytes,
    })
}

fn artifact_params_json(input: &SeedanceArtifactInput) -> serde_json::Value {
    let mut params = match &input.params_json {
        serde_json::Value::Object(map) => map.clone(),
        other => {
            let mut map = serde_json::Map::new();
            map.insert("params".to_string(), other.clone());
            map
        }
    };
    params.insert(
        "ark_task_id".to_string(),
        serde_json::Value::String(input.ark_task_id.clone()),
    );
    params.insert("usage".to_string(), input.usage.clone());
    params.insert(
        "source_video_url_sha256".to_string(),
        serde_json::Value::String(hex_sha256(input.video_url.as_bytes())),
    );
    serde_json::Value::Object(params)
}

fn hex_sha256(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::ark::ArkTransport;
    use crate::test_support::temp_db_path;
    use crate::tos::{TosConfig, TosRequest, TosResponse};

    struct ScriptedArk {
        responses: Vec<(u16, String)>,
    }

    impl ArkTransport for ScriptedArk {
        fn send(
            &mut self,
            method: &str,
            _url: &str,
            _headers: &BTreeMap<String, String>,
            _body: Option<&serde_json::Value>,
        ) -> std::result::Result<(u16, String), String> {
            assert!(matches!(method, "POST" | "GET"));
            Ok(self.responses.remove(0))
        }
    }

    #[derive(Default)]
    struct FakeDownloader;

    impl VideoDownloader for FakeDownloader {
        fn download(&mut self, url: &str) -> std::result::Result<Vec<u8>, String> {
            assert_eq!(url, "https://signed.example.test/video.mp4");
            Ok(b"fake mp4 bytes".to_vec())
        }
    }

    #[derive(Default)]
    struct FakeTos {
        last_request: Option<TosRequest>,
    }

    impl TosTransport for FakeTos {
        fn send(&mut self, request: TosRequest) -> std::result::Result<TosResponse, String> {
            self.last_request = Some(request);
            Ok(TosResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: Vec::new(),
            })
        }
    }

    fn seeded_db(name: &str) -> (std::path::PathBuf, Database) {
        let path = temp_db_path(name);
        let db = Database::open(&path).unwrap();
        db.create_project("p", 100.0).unwrap();
        db.create_chapter("c", "p", "{}").unwrap();
        db.create_scene("s", "c", "{}").unwrap();
        db.create_shot(
            "shot",
            "s",
            "{\"duration_s\":1}",
            None,
            None,
            false,
            "standard",
        )
        .unwrap();
        (path, db)
    }

    #[test]
    fn seedance_submit_uses_jobs_idempotency_layer() {
        let (path, mut db) = seeded_db("seedance-submit");
        let mut ark = ArkClient::new(
            "sk-test",
            ScriptedArk {
                responses: vec![(200, json!({"id": "ark-task-1"}).to_string())],
            },
        )
        .unwrap();
        let input = SeedanceSubmitInput {
            project_id: "p".to_string(),
            shot_id: "shot".to_string(),
            attempt: 0,
            model: "doubao-seedance-2-0-fast-260128".to_string(),
            payload: json!({"model": "doubao-seedance-2-0-fast-260128"}),
            reserve_cost: 1.0,
        };

        let first = submit_seedance_job_once(&mut db, &mut ark, &input).unwrap();
        let second = submit_seedance_job_once(&mut db, &mut ark, &input).unwrap();

        assert!(first.submitted_to_provider);
        assert!(!second.submitted_to_provider);
        assert_eq!(first.record.provider_job_id.as_deref(), Some("ark-task-1"));
        assert_eq!(first.record.id, second.record.id);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn seedance_poll_once_updates_running_then_succeeded_job_status() {
        let (path, mut db) = seeded_db("seedance-poll-success");
        let mut ark = ArkClient::new(
            "sk-test",
            ScriptedArk {
                responses: vec![
                    (200, json!({"status": "running"}).to_string()),
                    (
                        200,
                        json!({
                            "status": "succeeded",
                            "content": {"video_url": "https://signed.example.test/video.mp4"},
                            "usage": {"total_tokens": 42}
                        })
                        .to_string(),
                    ),
                ],
            },
        )
        .unwrap();
        let job = submit_job_once(
            db.connection_mut(),
            "p",
            "shot",
            0,
            &json!({"model": "doubao-seedance-2-0-fast-260128"}),
            "ark",
            "doubao-seedance-2-0-fast-260128",
            1.0,
            || Ok("ark-task-1".to_string()),
        )
        .unwrap();

        let running =
            poll_seedance_job_once(&mut db, &mut ark, "p", &job.record.id, "ark-task-1").unwrap();
        assert_eq!(
            running,
            SeedancePollOutcome::Running {
                status: "running".to_string()
            }
        );
        let status: String = db
            .connection()
            .query_row(
                "SELECT status FROM jobs WHERE id=?1",
                rusqlite::params![job.record.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "provider_running");

        let succeeded =
            poll_seedance_job_once(&mut db, &mut ark, "p", &job.record.id, "ark-task-1").unwrap();
        assert_eq!(
            succeeded,
            SeedancePollOutcome::Succeeded {
                video_url: "https://signed.example.test/video.mp4".to_string(),
                usage: json!({"total_tokens": 42}),
            }
        );
        let status: String = db
            .connection()
            .query_row(
                "SELECT status FROM jobs WHERE id=?1",
                rusqlite::params![job.record.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "provider_succeeded");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn seedance_poll_failure_releases_budget() {
        let (path, mut db) = seeded_db("seedance-poll-fail");
        let mut ark = ArkClient::new(
            "sk-test",
            ScriptedArk {
                responses: vec![(200, json!({"status": "failed"}).to_string())],
            },
        )
        .unwrap();
        let job = submit_job_once(
            db.connection_mut(),
            "p",
            "shot",
            0,
            &json!({"model": "doubao-seedance-2-0-fast-260128"}),
            "ark",
            "doubao-seedance-2-0-fast-260128",
            7.0,
            || Ok("ark-task-1".to_string()),
        )
        .unwrap();

        let outcome =
            poll_seedance_job_once(&mut db, &mut ark, "p", &job.record.id, "ark-task-1").unwrap();
        assert!(matches!(outcome, SeedancePollOutcome::Failed { .. }));
        let (reserved, status, reason): (f64, String, Option<String>) = db
            .connection()
            .query_row(
                "SELECT p.budget_reserved, j.status, j.failure_reason
                 FROM projects p JOIN jobs j ON j.id=?1
                 WHERE p.id='p'",
                rusqlite::params![job.record.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(reserved, 0.0);
        assert_eq!(status, "failed");
        assert!(reason.unwrap().contains("ended as failed"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn seedance_artifact_downloads_uploads_and_records_traceable_artifact() {
        let (path, db) = seeded_db("seedance-artifact");
        let tos_config = TosConfig::new(
            "ak",
            "sk",
            "https://tos-cn-beijing.volces.com",
            "bucket",
            "cn-beijing",
        )
        .unwrap();
        let mut tos = TosClient::new(tos_config, FakeTos::default());
        let mut downloader = FakeDownloader;
        let output = persist_seedance_video_artifact(
            &db,
            &mut tos,
            &mut downloader,
            &SeedanceArtifactInput {
                artifact_id: "artifact-shot".to_string(),
                shot_id: "shot".to_string(),
                tos_key: "projects/p/shot.mp4".to_string(),
                ark_task_id: "ark-task-1".to_string(),
                video_url: "https://signed.example.test/video.mp4".to_string(),
                usage: json!({"total_tokens": 42}),
                params_json: json!({"model": "doubao-seedance-2-0-fast-260128"}),
            },
        )
        .unwrap();

        assert_eq!(output.tos_uri, "tos://bucket/projects/p/shot.mp4");
        assert_eq!(output.size_bytes, b"fake mp4 bytes".len());
        let row: (String, String, String) = db
            .connection()
            .query_row(
                "SELECT tos_key, content_hash, params_json FROM artifacts WHERE id='artifact-shot'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row.0, "tos://bucket/projects/p/shot.mp4");
        assert!(row.1.starts_with("sha256:"));
        let params: serde_json::Value = serde_json::from_str(&row.2).unwrap();
        assert_eq!(params["ark_task_id"], "ark-task-1");
        assert_eq!(params["usage"]["total_tokens"], 42);
        assert!(params["source_video_url_sha256"].as_str().is_some());

        let _ = std::fs::remove_file(path);
    }
}
