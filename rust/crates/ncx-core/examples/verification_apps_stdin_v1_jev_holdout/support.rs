use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs::OpenOptions, io::Write, path::Path};

// serde_json normally overwrites repeated object keys. Preserve no ambiguous
// response: recursively reject duplicates before interpreting any numeric field.
pub struct Strict(pub Value);
impl<'de> serde::Deserialize<'de> for Strict {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Strict;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("unambiguous finite JSON")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Strict, E> {
                Ok(Strict(Value::Bool(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Strict, E> {
                serde_json::Number::from_f64(v)
                    .map(|v| Strict(Value::Number(v)))
                    .ok_or_else(|| E::custom("nonfinite"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Strict, A::Error> {
                let mut values = Vec::new();
                while let Some(Strict(v)) = seq.next_element()? {
                    values.push(v);
                }
                Ok(Strict(Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Strict, A::Error> {
                let mut result = serde_json::Map::new();
                while let Some((key, Strict(v))) = map.next_entry::<String, Strict>()? {
                    if result.insert(key, v).is_some() {
                        return Err(serde::de::Error::custom("duplicate key"));
                    }
                }
                Ok(Strict(Value::Object(result)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

pub fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key].as_str().ok_or_else(|| format!("missing_{key}"))
}
pub fn read(path: &Path, maximum: usize) -> Result<Vec<u8>, String> {
    let metadata = path
        .symlink_metadata()
        .map_err(|_| "original_file_missing")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > maximum as u64 {
        return Err("original_file_budget_or_link".into());
    }
    std::fs::read(path).map_err(|_| "original_read".into())
}
pub fn json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&read(path, 2_000_000)?).map_err(|_| "original_json".into())
}
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "exclusive_output_exists")?;
    file.write_all(bytes).map_err(|_| "output_write")?;
    file.sync_all().map_err(|_| "output_sync".into())
}
pub fn durable(path: &Path, value: &Value) -> Result<(), String> {
    write_new(path, &serde_json::to_vec(value).map_err(|_| "output_json")?)
}
pub fn event(path: &Path, value: &Value) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|_| "event_missing")?;
    file.write_all(value.to_string().as_bytes())
        .map_err(|_| "event_write")?;
    file.write_all(b"\n").map_err(|_| "event_write")?;
    file.sync_all().map_err(|_| "event_sync".into())
}
pub fn command(action: &str, path: &Path, reservation: Option<&Path>) -> Result<Value, String> {
    let mut command = std::process::Command::new("python");
    command.current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    for (key, _) in std::env::vars_os() {
        if ["KEY", "TOKEN", "SECRET"]
            .iter()
            .any(|word| key.to_string_lossy().to_uppercase().contains(word))
        {
            command.env_remove(key);
        }
    }
    command
        .args([
            "-m",
            "evals.verification.apps_stdin_v1.holdout_jev",
            action,
            "--prepared",
        ])
        .arg(path);
    if let Some(reserved) = reservation {
        command.arg("--reservation").arg(reserved);
    }
    let output = command
        .output()
        .map_err(|_| "fresh_original_proof_unavailable")?;
    if !output.status.success() {
        return Err("fresh_original_proof_rejected".into());
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "fresh_original_proof_output".into())
}
