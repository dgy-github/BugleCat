//! OpenAI-compatible local Marketplace manifests and source shapes.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const MARKETPLACE_PATHS: &[&str] = &[
    ".agents/plugins/marketplace.json",
    ".agents/plugins/api_marketplace.json",
    ".claude-plugin/marketplace.json",
    ".cursor-plugin/marketplace.json",
];

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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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
        #[serde(default)]
        sha: Option<String>,
    },
    Npm {
        package: String,
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        registry: Option<String>,
    },
}

impl<'de> Deserialize<'de> for MarketplaceSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawSource {
            Path(String),
            Object(RawSourceObject),
        }

        #[derive(Deserialize)]
        #[serde(tag = "source", rename_all = "lowercase")]
        enum RawSourceObject {
            Local {
                path: String,
            },
            #[serde(rename = "url", alias = "git")]
            Url {
                url: String,
                #[serde(default)]
                path: Option<String>,
                #[serde(default, rename = "ref")]
                ref_name: Option<String>,
                #[serde(default)]
                sha: Option<String>,
            },
            #[serde(rename = "git-subdir")]
            GitSubdir {
                url: String,
                path: String,
                #[serde(default, rename = "ref")]
                ref_name: Option<String>,
                #[serde(default)]
                sha: Option<String>,
            },
            Npm {
                package: String,
                #[serde(default)]
                version: Option<String>,
                #[serde(default)]
                registry: Option<String>,
            },
        }

        Ok(match RawSource::deserialize(deserializer)? {
            RawSource::Path(path) | RawSource::Object(RawSourceObject::Local { path }) => {
                Self::Local { path }
            }
            RawSource::Object(RawSourceObject::Url {
                url,
                path,
                ref_name,
                sha,
            }) => Self::Git {
                url,
                path,
                ref_name,
                sha,
            },
            RawSource::Object(RawSourceObject::GitSubdir {
                url,
                path,
                ref_name,
                sha,
            }) => Self::Git {
                url,
                path: Some(path),
                ref_name,
                sha,
            },
            RawSource::Object(RawSourceObject::Npm {
                package,
                version,
                registry,
            }) => Self::Npm {
                package,
                version,
                registry,
            },
        })
    }
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
