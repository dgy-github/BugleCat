//! Compatibility with OpenAI/Codex resource plugins and local marketplaces.

mod marketplace;
pub use marketplace::{
    discover_marketplaces, resolve_local_marketplace_plugin, Marketplace, MarketplacePlugin,
    MarketplaceSource,
};

use ncx_config::{HookConfig, McpServerConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST: &str = ".codex-plugin/plugin.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CodexPluginManifest {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub skills: ResourcePaths,
    #[serde(default)]
    pub mcp_servers: Option<Value>,
    #[serde(default)]
    pub apps: Option<Value>,
    #[serde(default)]
    pub hooks: Option<Value>,
    #[serde(default)]
    pub interface: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ResourcePaths {
    One(String),
    Many(Vec<String>),
    #[default]
    Missing,
}

impl ResourcePaths {
    pub(crate) fn values(&self) -> Vec<&str> {
        match self {
            Self::One(path) => vec![path],
            Self::Many(paths) => paths.iter().map(String::as_str).collect(),
            Self::Missing => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CodexPluginRecord {
    pub manifest: CodexPluginManifest,
    pub root: PathBuf,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CodexAppResource {
    pub plugin: String,
    pub name: String,
    pub connector_id: String,
}

impl CodexPluginRecord {
    pub fn skill_paths(&self) -> Vec<PathBuf> {
        let explicit = self
            .manifest
            .skills
            .values()
            .into_iter()
            .map(|path| self.root.join(path.strip_prefix("./").unwrap_or(path)))
            .collect::<Vec<_>>();
        if explicit.is_empty() {
            let conventional = self.root.join("skills");
            conventional
                .is_dir()
                .then_some(conventional)
                .into_iter()
                .collect()
        } else {
            explicit
        }
    }

    pub fn mcp_path(&self) -> Option<PathBuf> {
        self.root
            .join(".mcp.json")
            .is_file()
            .then(|| self.root.join(".mcp.json"))
    }

    pub fn apps_path(&self) -> Option<PathBuf> {
        self.manifest
            .apps
            .as_ref()
            .and_then(Value::as_str)
            .map(|path| self.root.join(path.strip_prefix("./").unwrap_or(path)))
            .or_else(|| {
                self.root
                    .join(".app.json")
                    .is_file()
                    .then(|| self.root.join(".app.json"))
            })
    }

    pub fn hooks_path(&self) -> Option<PathBuf> {
        self.root
            .join("hooks/hooks.json")
            .is_file()
            .then(|| self.root.join("hooks/hooks.json"))
    }
}

#[derive(Debug, Clone)]
pub struct CodexPluginCatalog {
    root: PathBuf,
}

pub(crate) fn discover_enabled_codex_plugins_with_home(
    workspace: &Path,
    home: Option<&Path>,
) -> Result<Vec<CodexPluginRecord>, String> {
    let mut plugins = std::collections::BTreeMap::new();
    let roots = home
        .into_iter()
        .map(|home| home.join(".ncx/codex-plugins"))
        .chain(std::iter::once(workspace.join(".ncx/codex-plugins")));
    for root in roots {
        let catalog = CodexPluginCatalog::new(root.clone());
        // Resource plugins are optional runtime inputs. A broken or
        // temporarily unreadable global catalog must not prevent workspace
        // plugins (or the built-in tools) from loading. `discover()` remains
        // strict for explicit catalog-management commands; this runtime path
        // deliberately isolates one catalog root as well as one plugin.
        let discovered = match catalog.discover_best_effort() {
            Ok(plugins) => plugins,
            Err(error) => {
                eprintln!("跳过无法读取 Codex 插件目录 '{}': {error}", root.display());
                continue;
            }
        };
        for plugin in discovered {
            if plugin.enabled {
                // Workspace plugins are visited last and intentionally shadow a
                // user-global plugin with the same stable name.
                plugins.insert(plugin.manifest.name.clone(), plugin);
            }
        }
    }
    Ok(plugins.into_values().collect())
}

fn local_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

impl CodexPluginCatalog {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn discover(&self) -> Result<Vec<CodexPluginRecord>, String> {
        self.discover_inner(false)
    }

    fn discover_best_effort(&self) -> Result<Vec<CodexPluginRecord>, String> {
        self.discover_inner(true)
    }

    fn discover_inner(&self, skip_invalid: bool) -> Result<Vec<CodexPluginRecord>, String> {
        if !self.root.is_dir() {
            return Ok(Vec::new());
        }
        if let Err(error) = self.recover_interrupted_updates() {
            if skip_invalid {
                eprintln!(
                    "跳过无法恢复的 Codex 插件目录 '{}': {error}",
                    self.root.display()
                );
            } else {
                return Err(error);
            }
        }
        let mut plugins = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(|error| error.to_string())? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if skip_invalid => {
                    eprintln!(
                        "跳过无法读取 Codex 插件目录条目 '{}': {error}",
                        self.root.display()
                    );
                    continue;
                }
                Err(error) => return Err(error.to_string()),
            };
            let is_dir = match entry.file_type() {
                Ok(kind) => kind.is_dir(),
                Err(error) if skip_invalid => {
                    eprintln!(
                        "跳过无法读取 Codex 插件条目 '{}': {error}",
                        entry.path().display()
                    );
                    continue;
                }
                Err(error) => return Err(error.to_string()),
            };
            if !is_dir {
                continue;
            }
            let root = entry.path();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if root.join(MANIFEST).is_file() {
                match load_record(root.clone()) {
                    Ok(plugin) => plugins.push(plugin),
                    Err(error) if skip_invalid => {
                        eprintln!("跳过损坏 Codex 插件 '{}': {error}", root.display())
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        plugins.sort_by(|left, right| left.manifest.name.cmp(&right.manifest.name));
        Ok(plugins)
    }

    pub fn install(&self, source: &Path) -> Result<CodexPluginRecord, String> {
        self.install_or_upgrade(source, false)
    }

    pub fn install_or_upgrade(
        &self,
        source: &Path,
        upgrade: bool,
    ) -> Result<CodexPluginRecord, String> {
        let source = source.canonicalize().map_err(|error| error.to_string())?;
        let record = load_record(source.clone())?;
        validate_segment(&record.manifest.name)?;
        fs::create_dir_all(&self.root).map_err(|error| error.to_string())?;
        self.recover_interrupted_updates()?;
        let target = self.root.join(&record.manifest.name);
        if target.exists() {
            if !upgrade {
                return Err(format!("插件 '{}' 已安装", record.manifest.name));
            }
            return self.replace(&source, &target, &record.manifest.name);
        }
        copy_resource_tree(&source, &target)?;
        load_record(target)
    }

    fn replace(
        &self,
        source: &Path,
        target: &Path,
        name: &str,
    ) -> Result<CodexPluginRecord, String> {
        let staging = self.root.join(format!(".{name}.staging"));
        let backup = self.root.join(format!(".{name}.backup"));
        if staging.exists() || backup.exists() {
            return Err(format!(
                "插件 '{name}' 存在未清理的升级目录，请先检查后重试"
            ));
        }
        copy_resource_tree(source, &staging)?;
        if let Err(error) = load_record(staging.clone()) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        let was_disabled = target.join(".disabled").exists();
        fs::rename(target, &backup).map_err(|error| error.to_string())?;
        if let Err(error) = fs::rename(&staging, target) {
            let _ = fs::rename(&backup, target);
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("插件升级替换失败: {error}"));
        }
        if was_disabled {
            fs::write(target.join(".disabled"), b"disabled\n")
                .map_err(|error| error.to_string())?;
        }
        fs::remove_dir_all(&backup).map_err(|error| error.to_string())?;
        load_record(target.to_path_buf())
    }

    pub fn uninstall(&self, name: &str) -> Result<(), String> {
        validate_segment(name)?;
        let target = self.root.join(name);
        if !target.is_dir() {
            return Err(format!("插件 '{name}' 尚未安装"));
        }
        fs::remove_dir_all(target).map_err(|error| error.to_string())
    }

    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), String> {
        validate_segment(name)?;
        let root = self.root.join(name);
        if !root.is_dir() {
            return Err(format!("插件 '{name}' 尚未安装"));
        }
        let marker = root.join(".disabled");
        if enabled && marker.exists() {
            fs::remove_file(marker).map_err(|error| error.to_string())
        } else if !enabled {
            fs::write(marker, b"disabled\n").map_err(|error| error.to_string())
        } else {
            Ok(())
        }
    }

    fn recover_interrupted_updates(&self) -> Result<(), String> {
        if !self.root.is_dir() {
            return Ok(());
        }
        let entries = fs::read_dir(&self.root)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .file_type()
                    .ok()
                    .filter(|kind| kind.is_dir())
                    .map(|_| entry)
            })
            .collect::<Vec<_>>();
        for entry in entries {
            let file_name = entry.file_name().to_string_lossy().to_string();
            let Some(name) = file_name
                .strip_prefix('.')
                .and_then(|value| value.strip_suffix(".backup"))
            else {
                continue;
            };
            validate_segment(name)?;
            let target = self.root.join(name);
            let backup = entry.path();
            let staging = self.root.join(format!(".{name}.staging"));
            if target.exists() {
                fs::remove_dir_all(&backup).map_err(|error| error.to_string())?;
            } else {
                fs::rename(&backup, &target)
                    .map_err(|error| format!("恢复插件 '{name}' 的升级备份失败: {error}"))?;
            }
            if staging.exists() {
                fs::remove_dir_all(staging).map_err(|error| error.to_string())?;
            }
        }
        for entry in fs::read_dir(&self.root)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
        {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with('.') && file_name.ends_with(".staging") {
                fs::remove_dir_all(entry.path()).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }
}

pub fn discover_codex_mcp_servers(workspace: &Path) -> Result<Vec<McpServerConfig>, String> {
    discover_codex_mcp_servers_with_home(workspace, local_home().as_deref())
}

pub(crate) fn discover_codex_mcp_servers_with_home(
    workspace: &Path,
    home: Option<&Path>,
) -> Result<Vec<McpServerConfig>, String> {
    let mut servers = Vec::new();
    for plugin in discover_enabled_codex_plugins_with_home(workspace, home)? {
        let value = match load_mcp_server_resource(&plugin) {
            Ok(Some(value)) => value,
            Ok(None) => continue,
            Err(error) => {
                eprintln!("跳过损坏 MCP 插件 '{}': {error}", plugin.manifest.name);
                continue;
            }
        };
        let Some(entries) = value.get("mcpServers").unwrap_or(&value).as_object() else {
            continue;
        };
        for (name, config) in entries {
            let command = config
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if command.is_empty() {
                continue;
            }
            let command = match resolve_mcp_process_value(&plugin.root, command) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!(
                        "跳过损坏 MCP server {}:{}: {error}",
                        plugin.manifest.name, name
                    );
                    continue;
                }
            };
            let args = match resolve_mcp_args(&plugin.root, config.get("args")) {
                Ok(args) => args,
                Err(error) => {
                    eprintln!(
                        "跳过损坏 MCP server {}:{}: {error}",
                        plugin.manifest.name, name
                    );
                    continue;
                }
            };
            let env = match resolve_mcp_env(config.get("env")) {
                Ok(env) => env,
                Err(error) => {
                    eprintln!(
                        "跳过损坏 MCP server {}:{}: {error}",
                        plugin.manifest.name, name
                    );
                    continue;
                }
            };
            servers.push(McpServerConfig {
                name: format!("{}:{name}", plugin.manifest.name),
                command,
                args,
                env,
            });
        }
    }
    Ok(servers)
}

fn load_mcp_server_resource(plugin: &CodexPluginRecord) -> Result<Option<Value>, String> {
    if let Some(value) = plugin.manifest.mcp_servers.clone() {
        return resolve_json_resource(plugin, "mcpServers", value).map(Some);
    }
    let Some(path) = plugin.mcp_path() else {
        return Ok(None);
    };
    serde_json::from_str(&fs::read_to_string(&path).map_err(|error| error.to_string())?)
        .map(Some)
        .map_err(|error| format!("无效 MCP 资源 {}: {error}", path.display()))
}

/// Resolve executable/script arguments that are explicitly relative paths
/// against the installed plugin root. Bare commands ("python", "npx",
/// "server") remain PATH lookups. This keeps installed resource plugins
/// independent from the directory they were copied from.
fn resolve_mcp_process_value(root: &Path, value: &str) -> Result<String, String> {
    let value = value.trim();
    let path = Path::new(value);
    let explicit_relative = value.starts_with("./")
        || value.starts_with("../")
        || value.starts_with(".\\")
        || value.starts_with("..\\");
    let path_like = explicit_relative;
    if !path_like || path.is_absolute() {
        return Ok(value.to_string());
    }
    Ok(validate_resource(root, value)?
        .to_string_lossy()
        .into_owned())
}

fn resolve_mcp_args(root: &Path, raw: Option<&Value>) -> Result<Vec<String>, String> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let Some(items) = raw.as_array() else {
        return Err("MCP args 必须是字符串数组".into());
    };
    items
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let value = value
                .as_str()
                .ok_or_else(|| format!("MCP args[{index}] 必须是字符串"))?;
            resolve_mcp_process_value(root, value)
        })
        .collect()
}

fn resolve_mcp_env(raw: Option<&Value>) -> Result<HashMap<String, String>, String> {
    let Some(raw) = raw else {
        return Ok(HashMap::new());
    };
    let Some(entries) = raw.as_object() else {
        return Err("MCP env 必须是字符串映射".into());
    };
    entries
        .iter()
        .map(|(key, value)| {
            let value = value
                .as_str()
                .ok_or_else(|| format!("MCP env['{key}'] 必须是字符串"))?;
            Ok((key.clone(), value.to_string()))
        })
        .collect()
}

/// Parse enabled plugin App declarations. Apps are hosted connector resources,
/// not local executables; callers may expose them for diagnostics and hand
/// their connector ids to an authenticated Apps gateway when one is present.
pub fn discover_codex_apps(workspace: &Path) -> Result<Vec<CodexAppResource>, String> {
    discover_codex_apps_with_home(workspace, local_home().as_deref())
}

pub(crate) fn discover_codex_apps_with_home(
    workspace: &Path,
    home: Option<&Path>,
) -> Result<Vec<CodexAppResource>, String> {
    let mut apps = Vec::new();
    for plugin in discover_enabled_codex_plugins_with_home(workspace, home)? {
        let Some(value) = (match load_apps_resource(&plugin) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("跳过损坏 Apps 插件 '{}': {error}", plugin.manifest.name);
                continue;
            }
        }) else {
            continue;
        };
        let Some(entries) = value.get("apps").unwrap_or(&value).as_object() else {
            eprintln!(
                "跳过损坏 Apps 插件 '{}': Apps 资源必须是对象",
                plugin.manifest.name
            );
            continue;
        };
        for (name, config) in entries {
            let Some(config) = config.as_object() else {
                eprintln!(
                    "跳过插件 '{}' 的 App '{}': 配置必须是对象",
                    plugin.manifest.name, name
                );
                continue;
            };
            let connector_id = config
                .get("id")
                .or_else(|| config.get("connector_id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if connector_id.is_empty() {
                eprintln!(
                    "跳过插件 '{}' 的 App '{}': 缺少 id/connector_id",
                    plugin.manifest.name, name
                );
                continue;
            }
            apps.push(CodexAppResource {
                plugin: plugin.manifest.name.clone(),
                name: name.clone(),
                connector_id: connector_id.to_string(),
            });
        }
    }
    Ok(apps)
}

pub fn discover_codex_hooks(workspace: &Path) -> Result<Vec<HookConfig>, String> {
    discover_codex_hooks_with_home(workspace, local_home().as_deref())
}

pub(crate) fn discover_codex_hooks_with_home(
    workspace: &Path,
    home: Option<&Path>,
) -> Result<Vec<HookConfig>, String> {
    let mut hooks = Vec::new();
    for plugin in discover_enabled_codex_plugins_with_home(workspace, home)? {
        let Some(value) = (match load_hooks_resource(&plugin) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("跳过损坏 Hooks 插件 '{}': {error}", plugin.manifest.name);
                continue;
            }
        }) else {
            continue;
        };
        let Some(events) = value.get("hooks").unwrap_or(&value).as_object() else {
            eprintln!(
                "跳过损坏 Hooks 插件 '{}': Hooks 资源必须是对象",
                plugin.manifest.name
            );
            continue;
        };
        for (event, groups) in events {
            let Some(event) = map_hook_event(event) else {
                continue;
            };
            for group in groups.as_array().into_iter().flatten() {
                let matcher = group
                    .get("matcher")
                    .and_then(Value::as_str)
                    .unwrap_or("*")
                    .to_string();
                for hook in group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if hook.get("type").and_then(Value::as_str) != Some("command") {
                        continue;
                    }
                    let command = hook
                        .get("command")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim();
                    if command.is_empty() {
                        continue;
                    }
                    hooks.push(HookConfig {
                        event: event.to_string(),
                        matcher: matcher.clone(),
                        command: command.to_string(),
                        timeout_s: hook
                            .get("timeout")
                            .and_then(Value::as_i64)
                            .unwrap_or(10)
                            .clamp(1, 300),
                    });
                }
            }
        }
    }
    Ok(hooks)
}

fn load_apps_resource(plugin: &CodexPluginRecord) -> Result<Option<Value>, String> {
    match plugin.manifest.apps.clone() {
        Some(value @ Value::String(_)) => resolve_json_resource(plugin, "Apps", value).map(Some),
        Some(value) => Ok(Some(value)),
        None => plugin
            .apps_path()
            .map(|path| read_json_resource_file(&path, "Apps"))
            .transpose(),
    }
}

fn load_hooks_resource(plugin: &CodexPluginRecord) -> Result<Option<Value>, String> {
    if let Some(value) = plugin.manifest.hooks.clone() {
        return resolve_hook_resources(plugin, value).map(Some);
    }
    plugin
        .hooks_path()
        .map(|path| read_json_resource_file(&path, "Hooks"))
        .transpose()
}

fn read_json_resource_file(path: &Path, field: &str) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&text)
        .map_err(|error| format!("无效 {field} 资源 {}: {error}", path.display()))
}

fn map_hook_event(event: &str) -> Option<&'static str> {
    match event {
        "PreToolUse" => Some("pre_tool"),
        "PostToolUse" => Some("post_tool"),
        "UserPromptSubmit" => Some("user_prompt"),
        "PreCompact" => Some("pre_compact"),
        "PostCompact" => Some("post_compact"),
        "Stop" => Some("stop"),
        _ => None,
    }
}

fn resolve_json_resource(
    plugin: &CodexPluginRecord,
    field: &str,
    value: Value,
) -> Result<Value, String> {
    let Value::String(path) = value else {
        return Ok(value);
    };
    let path = validate_resource(&plugin.root, &path)?;
    serde_json::from_str(&fs::read_to_string(&path).map_err(|error| error.to_string())?).map_err(
        |error| {
            format!(
                "插件 '{}' 的 {field} 资源无效: {error}",
                plugin.manifest.name
            )
        },
    )
}

fn resolve_hook_resources(plugin: &CodexPluginRecord, value: Value) -> Result<Value, String> {
    match value {
        Value::String(_) => resolve_json_resource(plugin, "hooks", value),
        Value::Array(documents) => {
            let mut merged = serde_json::Map::new();
            for document in documents {
                let document = resolve_json_resource(plugin, "hooks", document)?;
                let events = document
                    .get("hooks")
                    .unwrap_or(&document)
                    .as_object()
                    .ok_or_else(|| {
                        format!("插件 '{}' 的 hooks 资源必须是对象", plugin.manifest.name)
                    })?;
                for (event, groups) in events {
                    let target = merged
                        .entry(event.clone())
                        .or_insert_with(|| Value::Array(Vec::new()));
                    let Some(target) = target.as_array_mut() else {
                        return Err(format!(
                            "插件 '{}' 的 hooks 事件 '{event}' 无法合并",
                            plugin.manifest.name
                        ));
                    };
                    let Some(groups) = groups.as_array() else {
                        return Err(format!(
                            "插件 '{}' 的 hooks 事件 '{event}' 必须是数组",
                            plugin.manifest.name
                        ));
                    };
                    target.extend(groups.iter().cloned());
                }
            }
            Ok(serde_json::json!({ "hooks": merged }))
        }
        value => Ok(value),
    }
}

fn load_record(root: PathBuf) -> Result<CodexPluginRecord, String> {
    let text = fs::read_to_string(root.join(MANIFEST)).map_err(|error| error.to_string())?;
    let manifest: CodexPluginManifest =
        serde_json::from_str(&text).map_err(|error| format!("plugin.json 无效: {error}"))?;
    validate_manifest(&root, &manifest)?;
    Ok(CodexPluginRecord {
        enabled: !root.join(".disabled").exists(),
        manifest,
        root,
    })
}

fn validate_manifest(root: &Path, manifest: &CodexPluginManifest) -> Result<(), String> {
    validate_segment(&manifest.name)?;
    for path in manifest.skills.values() {
        validate_resource(root, path)?;
    }
    for path in [&manifest.apps] {
        if let Some(path) = path.as_ref().and_then(Value::as_str) {
            validate_resource(root, path)?;
        }
    }
    // MCP resources are deliberately checked by MCP discovery rather than at
    // plugin load time. A broken optional MCP server must not hide a plugin's
    // other resources or every other valid server in the session.
    for value in [&manifest.hooks].into_iter().flatten() {
        validate_json_resource_paths(root, value)?;
    }
    if let Some(interface) = manifest.interface.as_ref().and_then(Value::as_object) {
        for field in ["composerIcon", "logo", "logoDark"] {
            if let Some(path) = interface.get(field).and_then(Value::as_str) {
                validate_resource(root, path)?;
            }
        }
        if let Some(paths) = interface.get("screenshots").and_then(Value::as_array) {
            for path in paths.iter().filter_map(Value::as_str) {
                validate_resource(root, path)?;
            }
        }
    }
    for conventional in ["skills", ".mcp.json", ".app.json", "hooks/hooks.json"] {
        if root.join(conventional).exists() {
            validate_resource(root, conventional)?;
        }
    }
    Ok(())
}

fn validate_json_resource_paths(root: &Path, value: &Value) -> Result<(), String> {
    match value {
        Value::String(path) => {
            validate_resource(root, path)?;
        }
        Value::Array(values) => {
            for path in values.iter().filter_map(Value::as_str) {
                validate_resource(root, path)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_resource(root: &Path, value: &str) -> Result<PathBuf, String> {
    let relative = value.strip_prefix("./").unwrap_or(value);
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(format!("插件资源路径越界: {value}"));
    }
    let resolved = root
        .join(path)
        .canonicalize()
        .map_err(|_| format!("插件资源不存在: {value}"))?;
    let canonical_root = root.canonicalize().map_err(|error| error.to_string())?;
    if !resolved.starts_with(canonical_root) {
        return Err(format!("插件资源路径越界: {value}"));
    }
    Ok(resolved)
}

fn validate_segment(value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err("插件名称只能包含字母、数字、点、横线和下划线".to_string());
    }
    Ok(())
}

fn copy_resource_tree(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_symlink() {
            return Err("资源插件不得包含符号链接".to_string());
        }
        let destination = target.join(entry.file_name());
        if kind.is_dir() {
            copy_resource_tree(&entry.path(), &destination)?;
        } else {
            fs::copy(entry.path(), destination).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "openai_compat/tests.rs"]
mod tests;
