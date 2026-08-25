use super::ExternalPluginRecord;
use crate::tools::{Tool, ToolContext};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExternalProtocolRequest {
    Handshake {
        protocol: u32,
        host: String,
        plugin_id: String,
    },
    ToolCall {
        request_id: u64,
        tool: String,
        arguments: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExternalProtocolResponse {
    Handshake {
        protocol: u32,
        plugin_id: String,
        capabilities: Vec<String>,
        tools: Vec<ExternalToolDescriptor>,
    },
    ToolResult {
        request_id: u64,
        output: String,
        #[serde(default)]
        is_error: bool,
    },
    Error {
        request_id: Option<u64>,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExternalToolDescriptor {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExternalPluginHandshake {
    pub protocol: u32,
    pub plugin_id: String,
    pub capabilities: Vec<String>,
    pub tools: Vec<ExternalToolDescriptor>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternalPluginRegistration {
    pub plugin_id: String,
    pub tools: Vec<ExternalToolDescriptor>,
}

pub struct ExternalProcessTool {
    plugin: ExternalPluginRecord,
    descriptor: ExternalToolDescriptor,
}

impl ExternalProcessTool {
    pub fn new(plugin: ExternalPluginRecord, descriptor: ExternalToolDescriptor) -> Self {
        Self { plugin, descriptor }
    }
}

#[async_trait(?Send)]
impl Tool for ExternalProcessTool {
    fn name(&self) -> &str {
        &self.descriptor.name
    }

    fn description(&self) -> &str {
        &self.descriptor.description
    }

    fn parameters(&self) -> Value {
        self.descriptor.parameters.clone()
    }

    fn read_only(&self) -> bool {
        self.descriptor.read_only
    }

    async fn execute(&self, _ctx: &ToolContext, arguments: &Value) -> String {
        call_tool(
            &self.plugin,
            &self.descriptor.name,
            arguments.clone(),
            Duration::from_secs(30),
        )
        .unwrap_or_else(|error| format!("Error: 外部插件调用失败: {error}"))
    }
}

pub(super) fn handshake(
    plugin: &ExternalPluginRecord,
    timeout: Duration,
) -> Result<ExternalPluginRegistration, String> {
    let responses = exchange(
        plugin,
        &[ExternalProtocolRequest::Handshake {
            protocol: 1,
            host: "nanocodex".into(),
            plugin_id: plugin.manifest.id.clone(),
        }],
        timeout,
    )?;
    let handshake = responses
        .into_iter()
        .find_map(|response| match response {
            ExternalProtocolResponse::Handshake {
                protocol,
                plugin_id,
                capabilities,
                tools,
            } => Some(ExternalPluginHandshake {
                protocol,
                plugin_id,
                capabilities,
                tools,
            }),
            _ => None,
        })
        .ok_or_else(|| "插件未返回 handshake".to_string())?;
    validate_handshake(plugin, handshake)
}

fn call_tool(
    plugin: &ExternalPluginRecord,
    tool: &str,
    arguments: Value,
    timeout: Duration,
) -> Result<String, String> {
    let request_id = 1;
    let responses = exchange(
        plugin,
        &[
            ExternalProtocolRequest::Handshake {
                protocol: 1,
                host: "nanocodex".into(),
                plugin_id: plugin.manifest.id.clone(),
            },
            ExternalProtocolRequest::ToolCall {
                request_id,
                tool: tool.to_string(),
                arguments,
            },
        ],
        timeout,
    )?;
    for response in responses {
        match response {
            ExternalProtocolResponse::ToolResult {
                request_id: id,
                output,
                is_error,
            } if id == request_id => {
                return Ok(if is_error {
                    format!("Error: {output}")
                } else {
                    output
                });
            }
            ExternalProtocolResponse::Error {
                request_id: Some(id),
                message,
            } if id == request_id => return Err(message),
            _ => {}
        }
    }
    Err("插件未返回对应的 toolResult".into())
}

fn validate_handshake(
    plugin: &ExternalPluginRecord,
    handshake: ExternalPluginHandshake,
) -> Result<ExternalPluginRegistration, String> {
    if handshake.protocol != plugin.manifest.protocol {
        return Err(format!(
            "插件握手协议版本 {} 与清单 {} 不一致",
            handshake.protocol, plugin.manifest.protocol
        ));
    }
    if handshake.plugin_id != plugin.manifest.id {
        return Err("插件握手 ID 与清单不一致".into());
    }
    if handshake.capabilities != plugin.manifest.capabilities {
        return Err("插件握手能力与清单不一致".into());
    }
    let mut names = std::collections::HashSet::new();
    let prefix = format!("{}__", plugin.manifest.id.replace(['.', '-'], "_"));
    for tool in &handshake.tools {
        if tool.name.trim().is_empty() || tool.description.trim().is_empty() {
            return Err("外部插件工具名称和说明不能为空".into());
        }
        if !names.insert(tool.name.clone()) {
            return Err(format!("外部插件重复注册工具 '{}'", tool.name));
        }
        if !tool.name.starts_with(&prefix)
            || !tool
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(format!(
                "外部插件工具 '{}' 必须使用命名空间前缀 '{}' 且只含字母、数字和下划线",
                tool.name, prefix
            ));
        }
        if !tool.parameters.is_object() {
            return Err(format!(
                "外部插件工具 '{}' parameters 必须是对象",
                tool.name
            ));
        }
    }
    if !handshake.tools.is_empty() && !handshake.capabilities.iter().any(|value| value == "tool") {
        return Err("插件返回了工具但未声明 tool 能力".into());
    }
    Ok(ExternalPluginRegistration {
        plugin_id: handshake.plugin_id,
        tools: handshake.tools,
    })
}

fn exchange(
    plugin: &ExternalPluginRecord,
    requests: &[ExternalProtocolRequest],
    timeout: Duration,
) -> Result<Vec<ExternalProtocolResponse>, String> {
    let mut child = plugin.launch()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "插件 stdin 不可用".to_string())?;
    for request in requests {
        serde_json::to_writer(&mut stdin, request).map_err(|error| error.to_string())?;
        stdin.write_all(b"\n").map_err(|error| error.to_string())?;
    }
    drop(stdin);

    let started = Instant::now();
    loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(_) => break,
            None if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("插件响应超时（{} 秒）", timeout.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("插件进程退出失败: {}", stderr.trim()));
    }
    let stdout = String::from_utf8(output.stdout).map_err(|_| "插件输出不是 UTF-8".to_string())?;
    let responses = stdout
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect::<Vec<_>>();
    if responses.is_empty() {
        return Err("插件没有返回有效 JSON 协议消息".into());
    }
    Ok(responses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
    use serde_json::json;
    use std::fs;
    use std::io::Read;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ncx-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn fixture() -> ExternalPluginRecord {
        fixture_at(temp_root("protocol-plugin"))
    }

    fn fixture_at(root: PathBuf) -> ExternalPluginRecord {
        fs::create_dir_all(&root).unwrap();
        let executable = if cfg!(windows) {
            "plugin-test.exe"
        } else {
            "plugin-test"
        };
        fs::copy(std::env::current_exe().unwrap(), root.join(executable)).unwrap();
        ExternalPluginRecord {
            manifest: super::super::ExternalPluginManifest {
                id: "demo.echo".into(),
                name: "Echo".into(),
                version: "1.0.0".into(),
                protocol: 1,
                command: executable.into(),
                args: vec![
                    "--exact".into(),
                    "plugins::external::protocol::tests::protocol_fixture_worker".into(),
                    "--nocapture".into(),
                ],
                capabilities: vec!["tool".into()],
            },
            root,
            enabled: true,
        }
    }

    #[test]
    fn protocol_fixture_worker() {
        if std::env::var("NANOCODEX_PLUGIN_PROTOCOL").is_err() {
            return;
        }
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input).unwrap();
        for line in input.lines() {
            match serde_json::from_str::<ExternalProtocolRequest>(line).unwrap() {
                ExternalProtocolRequest::Handshake { .. } => println!(
                    "{}",
                    serde_json::to_string(&ExternalProtocolResponse::Handshake {
                        protocol: 1,
                        plugin_id: "demo.echo".into(),
                        capabilities: vec!["tool".into()],
                        tools: vec![ExternalToolDescriptor {
                            name: "demo_echo__echo".into(),
                            description: "Echo arguments".into(),
                            parameters: json!({"type":"object"}),
                            read_only: true,
                        }],
                    })
                    .unwrap()
                ),
                ExternalProtocolRequest::ToolCall {
                    request_id,
                    arguments,
                    ..
                } => println!(
                    "{}",
                    serde_json::to_string(&ExternalProtocolResponse::ToolResult {
                        request_id,
                        output: arguments.to_string(),
                        is_error: false,
                    })
                    .unwrap()
                ),
            }
        }
    }

    #[tokio::test]
    async fn handshake_registers_and_executes_a_real_isolated_tool() {
        let plugin = fixture();
        let registration = handshake(&plugin, Duration::from_secs(5)).unwrap();
        assert_eq!(registration.tools[0].name, "demo_echo__echo");
        let tool = ExternalProcessTool::new(plugin.clone(), registration.tools[0].clone());
        let workspace = PathBuf::from("external-protocol-test");
        let context = ToolContext::new(
            workspace.clone(),
            SandboxPolicy::new(WORKSPACE_WRITE, workspace),
        );
        assert_eq!(
            tool.execute(&context, &json!({"message":"你好"})).await,
            r#"{"message":"你好"}"#
        );
        let _ = fs::remove_dir_all(plugin.root);
    }

    #[tokio::test]
    async fn configured_runtime_discovers_and_registers_external_tools() {
        let workspace = temp_root("external-runtime");
        let plugin = fixture_at(workspace.join(".ncx/plugins/demo.echo"));
        fs::write(
            plugin.root.join("plugin.toml"),
            format!(
                "id = \"demo.echo\"\nname = \"Echo\"\nversion = \"1.0.0\"\nprotocol = 1\ncommand = \"{}\"\nargs = [\"--exact\", \"plugins::external::protocol::tests::protocol_fixture_worker\", \"--nocapture\"]\ncapabilities = [\"tool\"]\n",
                plugin.manifest.command
            ),
        )
        .unwrap();
        let context = ToolContext::new(
            workspace.clone(),
            SandboxPolicy::new(WORKSPACE_WRITE, &workspace),
        );
        let registry = crate::HarnessRuntimeBuilder::configured(&workspace)
            .unwrap()
            .build(context);
        assert!(registry.get("demo_echo__echo").is_some());
        assert_eq!(
            registry
                .execute("demo_echo__echo", &json!({"registered":true}))
                .await,
            r#"{"registered":true}"#
        );
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn handshake_rejects_tools_outside_the_plugin_namespace() {
        let plugin = fixture();
        let error = validate_handshake(
            &plugin,
            ExternalPluginHandshake {
                protocol: 1,
                plugin_id: "demo.echo".into(),
                capabilities: vec!["tool".into()],
                tools: vec![ExternalToolDescriptor {
                    name: "shell".into(),
                    description: "collision".into(),
                    parameters: json!({"type":"object"}),
                    read_only: false,
                }],
            },
        )
        .unwrap_err();
        assert!(error.contains("demo_echo__"), "{error}");
        let _ = fs::remove_dir_all(plugin.root);
    }
}
