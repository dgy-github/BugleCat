use serde_json::Value;

use crate::validation::assert_artifacts_passed;
use crate::{Result, VideoAgentError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Agent,
    DeterministicTool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentReasoningMode {
    SingleStructured,
    BoundedGenerateCritic,
    BoundedReact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSpec {
    pub node_id: String,
    pub kind: NodeKind,
    pub reasoning_mode: Option<AgentReasoningMode>,
    pub tools: Vec<String>,
    pub is_judgment_or_planning: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum P1AgentNode {
    Requirements,
    ScriptChapters,
    Storyboard,
    VisualAssets,
}

pub fn p1_agent_node_spec(node: P1AgentNode) -> NodeSpec {
    match node {
        P1AgentNode::Requirements => {
            agent_spec("requirements", AgentReasoningMode::SingleStructured, true)
        }
        P1AgentNode::ScriptChapters => agent_spec(
            "script_chapters",
            AgentReasoningMode::BoundedGenerateCritic,
            true,
        ),
        P1AgentNode::Storyboard => {
            agent_spec("storyboard", AgentReasoningMode::SingleStructured, true)
        }
        P1AgentNode::VisualAssets => {
            agent_spec("visual_assets", AgentReasoningMode::SingleStructured, false)
        }
    }
}

fn agent_spec(
    node_id: &str,
    reasoning_mode: AgentReasoningMode,
    is_judgment_or_planning: bool,
) -> NodeSpec {
    NodeSpec {
        node_id: node_id.to_string(),
        kind: NodeKind::Agent,
        reasoning_mode: Some(reasoning_mode),
        tools: Vec::new(),
        is_judgment_or_planning,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextPacket {
    pub stage: String,
    pub upstream_artifact_ids: Vec<String>,
    pub params_json: Value,
}

impl ContextPacket {
    pub fn new(
        stage: impl Into<String>,
        upstream_artifact_ids: Vec<String>,
        params_json: Value,
    ) -> Result<Self> {
        let packet = Self {
            stage: stage.into(),
            upstream_artifact_ids,
            params_json,
        };
        assert_no_reasoning_leak(&packet.params_json)?;
        Ok(packet)
    }
}

pub fn assert_context_packet_admissible(
    conn: &rusqlite::Connection,
    spec: &NodeSpec,
    packet: &ContextPacket,
) -> Result<()> {
    validate_node_spec(spec)?;
    assert_no_reasoning_leak(&packet.params_json)?;
    let refs = packet
        .upstream_artifact_ids
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_artifacts_passed(conn, &refs)?;
    Ok(())
}

fn validate_node_spec(spec: &NodeSpec) -> Result<()> {
    match spec.kind {
        NodeKind::Agent if spec.reasoning_mode.is_none() => {
            return Err(VideoAgentError::NodeContract(format!(
                "agent node {} must pin a reasoning mode",
                spec.node_id
            )));
        }
        NodeKind::DeterministicTool if spec.reasoning_mode.is_some() => {
            return Err(VideoAgentError::NodeContract(format!(
                "deterministic tool node {} must not set a reasoning mode",
                spec.node_id
            )));
        }
        _ => {}
    }

    if spec.is_judgment_or_planning && !spec.tools.is_empty() {
        return Err(VideoAgentError::NodeContract(format!(
            "judgment/planning node {} must have an empty tool set",
            spec.node_id
        )));
    }
    Ok(())
}

fn assert_no_reasoning_leak(value: &Value) -> Result<()> {
    let mut path = Vec::new();
    find_forbidden_reasoning_key(value, &mut path).map_or(Ok(()), |bad_path| {
        Err(VideoAgentError::NodeContract(format!(
            "Context Packet contains forbidden reasoning/history field at {bad_path}"
        )))
    })
}

fn find_forbidden_reasoning_key(value: &Value, path: &mut Vec<String>) -> Option<String> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if is_forbidden_context_key(key) {
                    path.push(key.clone());
                    return Some(render_path(path));
                }
                path.push(key.clone());
                if let Some(found) = find_forbidden_reasoning_key(child, path) {
                    return Some(found);
                }
                path.pop();
            }
            None
        }
        Value::Array(items) => {
            for (idx, child) in items.iter().enumerate() {
                path.push(idx.to_string());
                if let Some(found) = find_forbidden_reasoning_key(child, path) {
                    return Some(found);
                }
                path.pop();
            }
            None
        }
        _ => None,
    }
}

fn is_forbidden_context_key(key: &str) -> bool {
    matches!(
        key.trim().to_ascii_lowercase().as_str(),
        "reasoning"
            | "thought"
            | "thoughts"
            | "chain_of_thought"
            | "cot"
            | "conversation"
            | "conversation_history"
            | "messages"
            | "prompt_history"
            | "scratchpad"
    )
}

fn render_path(path: &[String]) -> String {
    if path.is_empty() {
        "$".to_string()
    } else {
        format!("$.{}", path.join("."))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::db::Database;
    use crate::test_support::temp_db_path;
    use crate::validation::{record_validation, ValidationInput};

    fn db_with_artifact(name: &str) -> (std::path::PathBuf, Database) {
        let path = temp_db_path(name);
        let db = Database::open(&path).expect("open db");
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
        db.create_artifact("a", Some("shot"), "brief", "tos://a", "hash", "{}")
            .unwrap();
        (path, db)
    }

    #[test]
    fn context_packet_rejects_reasoning_or_conversation_history() {
        let err = ContextPacket::new(
            "shots",
            vec![],
            json!({
                "artifact_ref": "a",
                "upstream": {"chain_of_thought": "hidden scratchpad"}
            }),
        )
        .expect_err("reasoning leaks are forbidden");
        assert!(err.to_string().contains("chain_of_thought"));
    }

    #[test]
    fn admissible_context_requires_upstream_pass_record() {
        let (path, db) = db_with_artifact("node-contract");
        let spec = NodeSpec {
            node_id: "shot_planner".to_string(),
            kind: NodeKind::Agent,
            reasoning_mode: Some(AgentReasoningMode::SingleStructured),
            tools: vec![],
            is_judgment_or_planning: true,
        };
        let packet =
            ContextPacket::new("shots", vec!["a".to_string()], json!({"budget_s": 5})).unwrap();

        assert!(matches!(
            assert_context_packet_admissible(db.connection(), &spec, &packet),
            Err(VideoAgentError::MissingPassingValidation { .. })
        ));

        record_validation(
            db.connection(),
            &ValidationInput {
                id: "v".to_string(),
                artifact_id: "a".to_string(),
                stage: "brief_l0".to_string(),
                gate_version: "v1".to_string(),
                verdict: "pass".to_string(),
                confidence: Some(1.0),
                aesthetic_score: None,
                layers_json: json!({}),
                escalate_reason: None,
            },
        )
        .unwrap();

        assert_context_packet_admissible(db.connection(), &spec, &packet).unwrap();
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn judgment_and_planning_nodes_have_no_tools_and_agents_pin_mode() {
        let tool_leak = NodeSpec {
            node_id: "judge".to_string(),
            kind: NodeKind::Agent,
            reasoning_mode: Some(AgentReasoningMode::SingleStructured),
            tools: vec!["download".to_string()],
            is_judgment_or_planning: true,
        };
        assert!(validate_node_spec(&tool_leak)
            .expect_err("judge tools forbidden")
            .to_string()
            .contains("empty tool set"));

        let unpinned = NodeSpec {
            node_id: "asset_agent".to_string(),
            kind: NodeKind::Agent,
            reasoning_mode: None,
            tools: vec![],
            is_judgment_or_planning: false,
        };
        assert!(validate_node_spec(&unpinned)
            .expect_err("agent mode must be pinned")
            .to_string()
            .contains("pin a reasoning mode"));
    }

    #[test]
    fn p1_agent_node_specs_pin_modes_and_have_no_tools() {
        let requirements = p1_agent_node_spec(P1AgentNode::Requirements);
        let chapters = p1_agent_node_spec(P1AgentNode::ScriptChapters);
        let storyboard = p1_agent_node_spec(P1AgentNode::Storyboard);
        let assets = p1_agent_node_spec(P1AgentNode::VisualAssets);

        assert_eq!(
            requirements.reasoning_mode,
            Some(AgentReasoningMode::SingleStructured)
        );
        assert_eq!(
            chapters.reasoning_mode,
            Some(AgentReasoningMode::BoundedGenerateCritic)
        );
        assert_eq!(
            storyboard.reasoning_mode,
            Some(AgentReasoningMode::SingleStructured)
        );
        assert_eq!(
            assets.reasoning_mode,
            Some(AgentReasoningMode::SingleStructured)
        );
        for spec in [requirements, chapters, storyboard, assets] {
            assert!(spec.tools.is_empty());
            validate_node_spec(&spec).unwrap();
        }
    }
}
