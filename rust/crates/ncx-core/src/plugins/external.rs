//! Discovery and lifecycle management for untrusted process-isolated plugins.
//!
//! Native dynamic libraries are intentionally rejected. External plugins are
//! declarative directories whose executable runs as a child process and can
//! only communicate through the versioned line-delimited JSON protocol.

mod protocol;

pub use protocol::{
    ExternalPluginHandshake, ExternalPluginRegistration, ExternalProcessTool,
    ExternalProtocolRequest, ExternalProtocolResponse, ExternalToolDescriptor,
};

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalPluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub protocol: u32,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExternalPluginRecord {
    pub manifest: ExternalPluginManifest,
    pub root: PathBuf,
    pub enabled: bool,
}

impl ExternalPluginRecord {
    /// Launch the plugin in a separate process with piped JSON protocol I/O.
    pub fn launch(&self) -> Result<Child, String> {
        if !self.enabled {
            return Err(format!("插件 '{}' 已停用", self.manifest.id));
        }
        validate_manifest(&self.manifest, &self.root)?;
        Command::new(self.root.join(&self.manifest.command))
            .args(&self.manifest.args)
            .current_dir(&self.root)
            .env_clear()
            .env(
                "NANOCODEX_PLUGIN_PROTOCOL",
                self.manifest.protocol.to_string(),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("启动隔离插件失败: {e}"))
    }

    /// Probe and validate the executable's protocol-v1 capability handshake.
    pub fn handshake(&self) -> Result<ExternalPluginRegistration, String> {
        protocol::handshake(self, Duration::from_secs(5))
    }

    /// Build model-facing tools only after a successful capability handshake.
    pub fn tools(&self) -> Result<Vec<Box<dyn crate::Tool>>, String> {
        let registration = self.handshake()?;
        Ok(registration
            .tools
            .into_iter()
            .map(|descriptor| {
                Box::new(ExternalProcessTool::new(self.clone(), descriptor)) as Box<dyn crate::Tool>
            })
            .collect())
    }
}

#[derive(Debug, Clone)]
pub struct ExternalPluginCatalog {
    root: PathBuf,
}

impl ExternalPluginCatalog {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn discover(&self) -> Result<Vec<ExternalPluginRecord>, String> {
        if !self.root.is_dir() {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                continue;
            }
            let dir = entry.path();
            let manifest_path = dir.join("plugin.toml");
            if !manifest_path.is_file() {
                continue;
            }
            let manifest = parse_manifest(&manifest_path)?;
            validate_manifest(&manifest, &dir)?;
            records.push(ExternalPluginRecord {
                enabled: !dir.join(".disabled").exists(),
                manifest,
                root: dir,
            });
        }
        records.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
        Ok(records)
    }

    pub fn install(&self, source: &Path) -> Result<ExternalPluginRecord, String> {
        let source = source
            .canonicalize()
            .map_err(|e| format!("插件源目录无效: {e}"))?;
        let manifest = parse_manifest(&source.join("plugin.toml"))?;
        validate_manifest(&manifest, &source)?;
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let target = self.root.join(&manifest.id);
        if target.exists() {
            return Err(format!("插件 '{}' 已安装；请使用 upgrade", manifest.id));
        }
        copy_dir(&source, &target)?;
        Ok(ExternalPluginRecord {
            manifest,
            root: target,
            enabled: true,
        })
    }

    pub fn upgrade(&self, source: &Path) -> Result<ExternalPluginRecord, String> {
        let source = source
            .canonicalize()
            .map_err(|e| format!("插件源目录无效: {e}"))?;
        let manifest = parse_manifest(&source.join("plugin.toml"))?;
        validate_manifest(&manifest, &source)?;
        let target = self.root.join(&manifest.id);
        if !target.is_dir() {
            return Err(format!("插件 '{}' 尚未安装", manifest.id));
        }
        let enabled = !target.join(".disabled").exists();
        let old = parse_manifest(&target.join("plugin.toml"))?;
        if version_tuple(&manifest.version)? <= version_tuple(&old.version)? {
            return Err("升级版本必须高于已安装版本".into());
        }
        let staging = self.root.join(format!(".{}.upgrade", manifest.id));
        if staging.exists() {
            fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
        }
        copy_dir(&source, &staging)?;
        if !enabled {
            fs::write(staging.join(".disabled"), b"disabled\n").map_err(|e| e.to_string())?;
        }
        let backup = self.root.join(format!(".{}.backup", manifest.id));
        if backup.exists() {
            fs::remove_dir_all(&backup).map_err(|e| e.to_string())?;
        }
        fs::rename(&target, &backup).map_err(|e| e.to_string())?;
        if let Err(error) = fs::rename(&staging, &target) {
            let _ = fs::rename(&backup, &target);
            return Err(error.to_string());
        }
        fs::remove_dir_all(&backup).map_err(|e| e.to_string())?;
        Ok(ExternalPluginRecord {
            manifest,
            root: target,
            enabled,
        })
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        validate_id(id)?;
        let dir = self.root.join(id);
        if !dir.is_dir() {
            return Err(format!("插件 '{id}' 尚未安装"));
        }
        let marker = dir.join(".disabled");
        if enabled {
            if marker.exists() {
                fs::remove_file(marker).map_err(|e| e.to_string())?;
            }
        } else {
            fs::write(marker, b"disabled\n").map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

fn parse_manifest(path: &Path) -> Result<ExternalPluginManifest, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("插件清单无效: {e}"))
}

fn validate_manifest(m: &ExternalPluginManifest, root: &Path) -> Result<(), String> {
    validate_id(&m.id)?;
    if m.name.trim().is_empty() || m.version.trim().is_empty() {
        return Err("插件名称和版本不能为空".into());
    }
    if m.protocol != 1 {
        return Err(format!("不支持插件协议版本 {}", m.protocol));
    }
    if m.capabilities.iter().any(|capability| capability != "tool") {
        return Err("插件协议 v1 仅支持声明 tool 能力".into());
    }
    let command = Path::new(&m.command);
    if command.is_absolute() || m.command.contains("..") {
        return Err("插件命令必须是插件目录内的相对路径".into());
    }
    let ext = command
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "dll" | "so" | "dylib") {
        return Err("禁止直接加载原生动态库；外部插件必须进程隔离".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let executable = root
        .join(command)
        .canonicalize()
        .map_err(|_| "插件命令文件不存在".to_string())?;
    if !executable.starts_with(&root) || !executable.is_file() {
        return Err("插件命令文件不存在".into());
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return Err("插件 ID 只能包含字母、数字、点和连字符".into());
    }
    Ok(())
}

fn version_tuple(value: &str) -> Result<Vec<u64>, String> {
    value
        .split('.')
        .map(|p| {
            p.parse::<u64>()
                .map_err(|_| "版本必须是数字点分格式".into())
        })
        .collect()
}

fn copy_dir(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_symlink() {
            return Err("插件包不得包含符号链接".into());
        }
        let destination = target.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_dir(&entry.path(), &destination)?;
        } else {
            fs::copy(entry.path(), destination).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ncx-plugin-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn fixture(root: &Path, version: &str, command: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join(command), b"echo plugin").unwrap();
        fs::write(root.join("plugin.toml"), format!(
            "id = \"demo.echo\"\nname = \"Echo\"\nversion = \"{version}\"\nprotocol = 1\ncommand = \"{command}\"\ncapabilities = [\"tool\"]\n"
        )).unwrap();
    }

    #[test]
    fn install_disable_enable_and_upgrade_are_atomic() {
        let root = temp("catalog");
        let v1 = temp("v1");
        let v2 = temp("v2");
        fixture(&v1, "1.0.0", "run.cmd");
        fixture(&v2, "1.1.0", "run.cmd");
        let catalog = ExternalPluginCatalog::new(&root);
        assert_eq!(catalog.install(&v1).unwrap().manifest.version, "1.0.0");
        catalog.set_enabled("demo.echo", false).unwrap();
        assert!(!catalog.discover().unwrap()[0].enabled);
        catalog.set_enabled("demo.echo", true).unwrap();
        assert_eq!(catalog.upgrade(&v2).unwrap().manifest.version, "1.1.0");
        assert!(catalog.discover().unwrap()[0].enabled);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(v1);
        let _ = fs::remove_dir_all(v2);
    }

    #[test]
    fn native_library_and_path_escape_are_rejected() {
        let root = temp("unsafe");
        fixture(&root, "1.0.0", "plugin.dll");
        assert!(ExternalPluginCatalog::new(temp("target"))
            .install(&root)
            .unwrap_err()
            .contains("动态库"));
        fs::write(root.join("plugin.toml"), "id=\"demo.echo\"\nname=\"Echo\"\nversion=\"1.0.0\"\nprotocol=1\ncommand=\"../run.exe\"\n").unwrap();
        assert!(ExternalPluginCatalog::new(temp("target2"))
            .install(&root)
            .unwrap_err()
            .contains("相对路径"));
        let _ = fs::remove_dir_all(root);
    }
}
