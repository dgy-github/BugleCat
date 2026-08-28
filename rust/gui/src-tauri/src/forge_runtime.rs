use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug)]
pub struct ForgeRuntimePaths {
    pub root: PathBuf,
    pub python: PathBuf,
    pub script: PathBuf,
    pub agent: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForgeRuntimeManifest {
    schema: String,
    python_sha256: String,
    ncx_sha256: String,
    forge_sha256: String,
}

pub fn discover(resource_dir: &Path) -> Result<ForgeRuntimePaths, String> {
    let packaged = resource_dir.join("forge-runtime");
    if packaged.join("manifest.json").is_file() {
        return validate(&packaged);
    }
    if cfg!(debug_assertions) {
        let development = Path::new(env!("CARGO_MANIFEST_DIR")).join("forge-runtime");
        if development.join("manifest.json").is_file() {
            return validate(&development);
        }
    }
    Err("Forge 运行时未安装；请使用包含 Forge 资源的完整安装包".into())
}

fn validate(root: &Path) -> Result<ForgeRuntimePaths, String> {
    let bytes = std::fs::read(root.join("manifest.json"))
        .map_err(|_| "Forge 运行时清单不可读".to_string())?;
    // Windows PowerShell 5 writes UTF-8 with a BOM by default. Existing staged
    // runtimes remain valid while newly generated manifests are written BOM-free.
    let json = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    let manifest: ForgeRuntimeManifest =
        serde_json::from_slice(json).map_err(|_| "Forge 运行时清单无效".to_string())?;
    if manifest.schema != "buglecat-forge-runtime/v1" {
        return Err("Forge 运行时版本不兼容".into());
    }
    let python = verified_file(root, "python/python.exe", &manifest.python_sha256)?;
    let script = verified_file(root, "train/forge.py", &manifest.forge_sha256)?;
    let agent = verified_file(root, "bin/ncx.exe", &manifest.ncx_sha256)?;
    Ok(ForgeRuntimePaths {
        root: root.to_path_buf(),
        python,
        script,
        agent,
    })
}

fn verified_file(root: &Path, relative: &str, expected: &str) -> Result<PathBuf, String> {
    let path = root.join(relative);
    if !path.is_file() {
        return Err(format!("Forge 运行时缺少 {relative}"));
    }
    let actual = sha256(&path).map_err(|_| format!("Forge 运行时无法校验 {relative}"))?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!("Forge 运行时校验失败：{relative}"));
    }
    Ok(path)
}

fn sha256(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:X}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ncx-forge-runtime-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for relative in ["python/python.exe", "train/forge.py", "bin/ncx.exe"] {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, relative.as_bytes()).unwrap();
        }
        let manifest = serde_json::json!({
            "schema": "buglecat-forge-runtime/v1",
            "pythonVersion": "test",
            "pythonSha256": sha256(&root.join("python/python.exe")).unwrap(),
            "ncxSha256": sha256(&root.join("bin/ncx.exe")).unwrap(),
            "forgeSha256": sha256(&root.join("train/forge.py")).unwrap(),
        });
        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        root
    }

    #[test]
    fn valid_runtime_resolves_all_owned_files() {
        let root = fixture("valid");
        let paths = validate(&root).unwrap();
        assert_eq!(paths.root, root);
        assert!(paths.python.is_file());
        assert!(paths.script.is_file());
        assert!(paths.agent.is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tampered_sidecar_is_rejected() {
        let root = fixture("tampered");
        std::fs::write(root.join("bin/ncx.exe"), b"changed").unwrap();
        let error = validate(&root).unwrap_err();
        assert!(error.contains("校验失败"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn accepts_legacy_powershell_utf8_bom_manifest() {
        let root = fixture("bom");
        let manifest = std::fs::read(root.join("manifest.json")).unwrap();
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend(manifest);
        std::fs::write(root.join("manifest.json"), with_bom).unwrap();
        assert!(validate(&root).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }
}
