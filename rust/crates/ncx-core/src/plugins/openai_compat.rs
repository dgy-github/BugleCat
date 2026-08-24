//! Compatibility with OpenAI/Codex resource plugins and local marketplaces.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST: &str = ".codex-plugin/plugin.json";
const MARKETPLACE_PATHS: &[&str] = &[
    ".agents/plugins/marketplace.json",
    ".agents/plugins/api_marketplace.json",
    ".claude-plugin/marketplace.json",
    ".cursor-plugin/marketplace.json",
];

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
    pub apps: Option<String>,
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
    fn values(&self) -> Vec<&str> {
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

#[derive(Debug, Clone)]
pub struct CodexPluginCatalog {
    root: PathBuf,
}

impl CodexPluginCatalog {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn discover(&self) -> Result<Vec<CodexPluginRecord>, String> {
        if !self.root.is_dir() {
            return Ok(Vec::new());
        }
        let mut plugins = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            if !entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_dir()
            {
                continue;
            }
            let root = entry.path();
            if root.join(MANIFEST).is_file() {
                plugins.push(load_record(root)?);
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Marketplace {
    pub name: String,
    #[serde(default)]
    pub plugins: Vec<MarketplacePlugin>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplacePlugin {
    pub name: String,
    pub source: MarketplaceSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum MarketplaceSource {
    Local {
        path: String,
    },
    Git {
        url: String,
        #[serde(default)]
        path: Option<String>,
        #[serde(default, rename = "ref")]
        ref_name: Option<String>,
    },
    Npm {
        package: String,
        #[serde(default)]
        version: Option<String>,
    },
}

pub fn discover_marketplaces(root: &Path) -> Result<Vec<(PathBuf, Marketplace)>, String> {
    let mut found = Vec::new();
    for relative in MARKETPLACE_PATHS {
        let path = root.join(relative);
        if !path.is_file() {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        let marketplace = serde_json::from_str(&text)
            .map_err(|error| format!("无效 Marketplace {}: {error}", path.display()))?;
        found.push((path, marketplace));
    }
    Ok(found)
}

pub fn resolve_local_marketplace_plugin(
    marketplace_path: &Path,
    plugin: &MarketplacePlugin,
) -> Result<PathBuf, String> {
    let MarketplaceSource::Local { path } = &plugin.source else {
        return Err("远程 Git/NPM Marketplace 需要先物化到本地缓存".to_string());
    };
    let relative = Path::new(path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("Marketplace 本地插件路径不得越界".to_string());
    }
    let relative_layout = MARKETPLACE_PATHS
        .iter()
        .find(|relative| marketplace_path.ends_with(relative))
        .ok_or_else(|| "Marketplace 路径不在支持的清单位置".to_string())?;
    let mut base = marketplace_path.to_path_buf();
    for _ in Path::new(relative_layout).components() {
        base.pop();
    }
    let resolved = base
        .join(relative)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let base = base.canonicalize().map_err(|error| error.to_string())?;
    if !resolved.starts_with(base) {
        return Err("Marketplace 本地插件路径不得越界".to_string());
    }
    Ok(resolved)
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
        if let Some(path) = path.as_deref() {
            validate_resource(root, path)?;
        }
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
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp(name: &str) -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ncx-codex-plugin-{name}-{id}"))
    }

    fn fixture(root: &Path, name: &str) {
        fs::create_dir_all(root.join(".codex-plugin")).unwrap();
        fs::create_dir_all(root.join("skills")).unwrap();
        fs::write(root.join("skills/SKILL.md"), "---\nname: demo\n---\n").unwrap();
        fs::write(
            root.join(MANIFEST),
            format!(r#"{{"name":"{name}","skills":["./skills/SKILL.md"]}}"#),
        )
        .unwrap();
    }

    #[test]
    fn codex_plugin_installs_discovers_toggles_and_uninstalls() {
        let source = temp("source");
        let target = temp("target");
        fixture(&source, "demo.plugin");
        let catalog = CodexPluginCatalog::new(&target);
        assert_eq!(
            catalog.install(&source).unwrap().manifest.name,
            "demo.plugin"
        );
        catalog.set_enabled("demo.plugin", false).unwrap();
        assert!(!catalog.discover().unwrap()[0].enabled);
        catalog.uninstall("demo.plugin").unwrap();
        assert!(catalog.discover().unwrap().is_empty());
    }

    #[test]
    fn codex_plugin_upgrade_replaces_resources_and_preserves_disabled_state() {
        let source = temp("upgrade-source");
        let target = temp("upgrade-target");
        fixture(&source, "demo.plugin");
        let catalog = CodexPluginCatalog::new(&target);
        catalog.install(&source).unwrap();
        catalog.set_enabled("demo.plugin", false).unwrap();
        fs::write(
            source.join(MANIFEST),
            r#"{"name":"demo.plugin","version":"2"}"#,
        )
        .unwrap();
        let upgraded = catalog.install_or_upgrade(&source, true).unwrap();
        assert_eq!(upgraded.manifest.version.as_deref(), Some("2"));
        assert!(!upgraded.enabled);
    }

    #[test]
    fn plugin_and_marketplace_paths_cannot_escape_their_roots() {
        let source = temp("escape");
        fs::create_dir_all(source.join(".codex-plugin")).unwrap();
        fs::write(
            source.join(MANIFEST),
            r#"{"name":"bad","skills":"../secret"}"#,
        )
        .unwrap();
        assert!(load_record(source).unwrap_err().contains("越界"));
    }

    #[test]
    fn local_marketplace_sources_resolve_from_repository_root_for_all_layouts() {
        for layout in [
            ".agents/plugins/marketplace.json",
            ".claude-plugin/marketplace.json",
        ] {
            let root = temp("marketplace-layout");
            let manifest = root.join(layout);
            fs::create_dir_all(manifest.parent().unwrap()).unwrap();
            let plugin_root = root.join("plugins/demo");
            fixture(&plugin_root, "demo");
            fs::write(&manifest, r#"{"name":"demo","plugins":[]}"#).unwrap();
            let plugin = MarketplacePlugin {
                name: "demo".into(),
                source: MarketplaceSource::Local {
                    path: "./plugins/demo".into(),
                },
            };
            assert_eq!(
                resolve_local_marketplace_plugin(&manifest, &plugin).unwrap(),
                plugin_root.canonicalize().unwrap()
            );
            let _ = fs::remove_dir_all(root);
        }
    }
}
