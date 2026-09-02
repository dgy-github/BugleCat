//! File-driven Profile, Bundle and Overlay composition.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml::Value;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PluginEntry {
    pub id: String,
    pub plugin: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default = "empty_config")]
    pub config: Value,
}

fn enabled_by_default() -> bool {
    true
}

fn empty_config() -> Value {
    Value::Table(Default::default())
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BundleSpec {
    pub id: String,
    #[serde(default)]
    pub plugins: Vec<PluginEntry>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileSpec {
    pub name: String,
    pub bundles: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OverlayEntry {
    pub id: String,
    pub plugin: Option<String>,
    pub enabled: Option<bool>,
    pub config: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OverlaySpec {
    #[serde(default)]
    pub plugins: Vec<OverlayEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HarnessComposition {
    pub profile: String,
    pub entries: Vec<PluginEntry>,
}

impl HarnessComposition {
    pub fn load(
        root: &Path,
        profile_name: &str,
        overlay_paths: &[PathBuf],
    ) -> Result<Self, String> {
        validate_file_id("profile", profile_name)?;
        let profile: ProfileSpec =
            read_toml(&root.join("profiles").join(format!("{profile_name}.toml")))?;
        if profile.name != profile_name {
            return Err(format!(
                "harness profile file declares '{}', expected '{profile_name}'",
                profile.name
            ));
        }
        let mut bundles = Vec::new();
        for bundle_id in &profile.bundles {
            validate_file_id("bundle", bundle_id)?;
            bundles.push(read_toml(
                &root.join("bundles").join(format!("{bundle_id}.toml")),
            )?);
        }
        let overlays = overlay_paths
            .iter()
            .map(|path| read_toml(path))
            .collect::<Result<Vec<_>, _>>()?;
        Self::compose(profile_name, profile, bundles, overlays)
    }

    pub fn compose(
        profile_name: &str,
        profile: ProfileSpec,
        bundles: Vec<BundleSpec>,
        overlays: Vec<OverlaySpec>,
    ) -> Result<Self, String> {
        if profile.name != profile_name {
            return Err(format!(
                "harness profile declares '{}', expected '{profile_name}'",
                profile.name
            ));
        }
        validate_file_id("profile", &profile.name)?;
        if profile.bundles.len() != bundles.len() {
            return Err(format!(
                "harness profile '{}' requires {} bundle(s), received {}",
                profile.name,
                profile.bundles.len(),
                bundles.len()
            ));
        }
        let mut entries = Vec::new();
        let mut ids = HashSet::new();
        for (bundle_id, bundle) in profile.bundles.iter().zip(bundles) {
            validate_file_id("bundle", bundle_id)?;
            validate_file_id("bundle", &bundle.id)?;
            if bundle.id != *bundle_id {
                return Err(format!(
                    "harness bundle file declares '{}', expected '{bundle_id}'",
                    bundle.id
                ));
            }
            for entry in bundle.plugins {
                validate_entry(&entry)?;
                if !ids.insert(entry.id.clone()) {
                    return Err(format!("duplicate harness entry id '{}'", entry.id));
                }
                entries.push(entry);
            }
        }
        for overlay in overlays {
            apply_overlay(&mut entries, overlay)?;
        }
        Ok(Self {
            profile: profile.name,
            entries,
        })
    }

    pub fn enabled_plugins(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter(|entry| entry.enabled)
            .map(|entry| entry.plugin.as_str())
    }

    pub fn enabled_entries(&self) -> impl Iterator<Item = &PluginEntry> {
        self.entries.iter().filter(|entry| entry.enabled)
    }

    pub fn apply_overlay_files(mut self, paths: &[PathBuf]) -> Result<Self, String> {
        for path in paths {
            apply_overlay(&mut self.entries, read_toml(path)?)?;
        }
        Ok(self)
    }
}

fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read harness config '{}': {error}", path.display()))?;
    toml::from_str(&text)
        .map_err(|error| format!("invalid harness config '{}': {error}", path.display()))
}

fn validate_entry(entry: &PluginEntry) -> Result<(), String> {
    if entry.id.trim().is_empty() {
        return Err("harness entry id must not be empty".to_string());
    }
    if entry.plugin.trim().is_empty() {
        return Err(format!(
            "harness entry '{}' has an empty plugin id",
            entry.id
        ));
    }
    Ok(())
}

fn validate_file_id(kind: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(format!("invalid harness {kind} id '{value}'"));
    }
    Ok(())
}

fn apply_overlay(entries: &mut [PluginEntry], overlay: OverlaySpec) -> Result<(), String> {
    let positions = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    for patch in overlay.plugins {
        if !seen.insert(patch.id.clone()) {
            return Err(format!("duplicate harness overlay entry '{}'", patch.id));
        }
        let index = positions
            .get(&patch.id)
            .copied()
            .ok_or_else(|| format!("unknown harness overlay entry '{}'", patch.id))?;
        let current = &mut entries[index];
        if let Some(plugin) = patch.plugin {
            current.plugin = plugin;
        }
        if let Some(enabled) = patch.enabled {
            current.enabled = enabled;
        }
        if let Some(config) = patch.config {
            current.config = config;
        }
        validate_entry(current)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let root = crate::test_support::unique_temp_dir("ncx-harness-composition");
        fs::create_dir_all(root.join("profiles")).unwrap();
        fs::create_dir_all(root.join("bundles")).unwrap();
        fs::write(
            root.join("profiles/full.toml"),
            "name = \"full\"\nbundles = [\"base\", \"process\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("bundles/base.toml"),
            "id = \"base\"\n[[plugins]]\nid = \"core\"\nplugin = \"ncx.core\"\n",
        )
        .unwrap();
        fs::write(
            root.join("bundles/process.toml"),
            "id = \"process\"\n[[plugins]]\nid = \"process\"\nplugin = \"ncx.process\"\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn profile_stacks_bundles_and_overlay_replaces_by_entry_id() {
        let root = fixture();
        let overlay = root.join("readonly.toml");
        fs::write(&overlay, "[[plugins]]\nid = \"process\"\nenabled = false\n").unwrap();
        let composition = HarnessComposition::load(&root, "full", &[overlay]).unwrap();
        assert_eq!(
            composition.enabled_plugins().collect::<Vec<_>>(),
            vec!["ncx.core"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_overlay_entry_fails_loud() {
        let root = fixture();
        let overlay = root.join("bad.toml");
        fs::write(&overlay, "[[plugins]]\nid = \"missing\"\nenabled = false\n").unwrap();
        let error = HarnessComposition::load(&root, "full", &[overlay]).unwrap_err();
        assert!(error.contains("unknown harness overlay entry 'missing'"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn profile_and_bundle_ids_cannot_escape_the_config_root() {
        let root = fixture();
        let error = HarnessComposition::load(&root, "../outside", &[]).unwrap_err();
        assert!(error.contains("invalid harness profile id"));
        fs::remove_dir_all(root).unwrap();
    }
}
