//! Transparent schema auto-migrations for vault YAML files.
//!
//! Vault YAML files carry a top-level `version` field. On read, if the
//! on-disk version is older than [`CURRENT_VERSION`], a migration ladder
//! upgrades the document in memory and rewrites it to disk atomically.
//!
//! [`read_yaml_typed`] adds a typed fallback: if strict Serde deserialization
//! fails (e.g. missing required fields, wrong types), it falls back to a
//! `serde_json::Value`-based approach, injects caller-provided defaults for
//! absent keys, writes the patched file back silently, and retries.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use serde_json::Value as JVal;
use serde_yaml::Value as YVal;

use crate::core::vaultio::atomic_write;

/// The schema version this binary expects for all vault YAMLs.
pub const CURRENT_VERSION: u64 = 1;

/// Read a YAML file and, if older than [`CURRENT_VERSION`], migrate + overwrite.
/// Returns the up-to-date raw string.
pub fn read_and_migrate(file: &Path) -> Result<String> {
    let raw = fs::read_to_string(file)
        .with_context(|| format!("reading {}", file.display()))?;
    let mut doc: YVal = match serde_yaml::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Ok(raw), // not YAML / empty: let the caller fail in context
    };

    let on_disk = doc.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    if on_disk >= CURRENT_VERSION {
        return Ok(raw);
    }

    let mut v = on_disk;
    while v < CURRENT_VERSION {
        migrate_step(&mut doc, v)?;
        v += 1;
    }
    if let YVal::Mapping(ref mut map) = doc {
        map.insert(YVal::String("version".into()), YVal::Number(CURRENT_VERSION.into()));
    }
    let out = serde_yaml::to_string(&doc)?;
    atomic_write(file, out.as_bytes())?;
    eprintln!("[migrate] {}: v{} → v{}", file.display(), on_disk, CURRENT_VERSION);
    Ok(out)
}

/// Apply one migration step from `from` → `from+1`.
fn migrate_step(_doc: &mut YVal, _from: u64) -> Result<()> {
    Ok(())
}

// ── Typed fallback with default injection ─────────────────────────────────────

/// Read a vault YAML file and deserialize it as `T`.
///
/// **Fast path**: `read_and_migrate` → strict `serde_yaml::from_str::<T>`.
///
/// **Fallback** (strict fails): parse as `serde_json::Value`, deep-merge
/// `defaults` for any absent keys, write the patched YAML back to disk
/// silently, then deserialize from the patched value.
///
/// This makes any vault YAML forward- and backward-compatible: old files with
/// missing fields are patched on first read without any manual migration step.
pub fn read_yaml_typed<T>(file: &Path, defaults: &JVal) -> Result<T>
where
    T: DeserializeOwned,
{
    let raw = read_and_migrate(file)?;

    // Fast path: strict typed deserialization.
    if let Ok(v) = serde_yaml::from_str::<T>(&raw) {
        return Ok(v);
    }

    // Fallback: build a JSON value, inject missing defaults, patch + retry.
    eprintln!(
        "[migrate] {}: schema mismatch — injecting defaults",
        file.display()
    );

    let yaml_val: YVal = serde_yaml::from_str(&raw)
        .unwrap_or_else(|_| YVal::Mapping(Default::default()));

    let mut json_val: JVal = serde_json::to_value(&yaml_val)
        .unwrap_or_else(|_| JVal::Object(Default::default()));

    inject_defaults(&mut json_val, defaults);

    // Serialize the patched doc back to YAML and write silently.
    let patched_yaml: YVal = serde_yaml::to_value(&json_val)
        .map_err(|e| anyhow!("re-serializing {}: {e}", file.display()))?;
    let out = serde_yaml::to_string(&patched_yaml)
        .map_err(|e| anyhow!("serializing patched {}: {e}", file.display()))?;
    atomic_write(file, out.as_bytes())?;

    // Final deserialization from the now-complete JSON value.
    serde_json::from_value(json_val)
        .map_err(|e| anyhow!("deserializing patched {}: {e}", file.display()))
}

/// Deep-merge `defaults` into `target` for absent or structurally-mismatched keys.
///
/// Rules:
/// - Key absent → insert default value.
/// - Key present as object, default is object → recurse.
/// - Key present as non-object but default expects an object → replace with default
///   (handles old YAMLs where a mapping field was written as a scalar).
/// - Key present as scalar, default is scalar → keep existing (existing value wins).
pub fn inject_defaults(target: &mut JVal, defaults: &JVal) {
    let (JVal::Object(t), JVal::Object(d)) = (target, defaults) else {
        return;
    };
    for (k, dv) in d {
        match t.get_mut(k) {
            None => {
                t.insert(k.clone(), dv.clone());
            }
            Some(tv) if tv.is_object() && dv.is_object() => {
                inject_defaults(tv, dv);
            }
            Some(tv) if !tv.is_object() && dv.is_object() => {
                // Scalar where a mapping is expected → replace with default mapping.
                *tv = dv.clone();
            }
            _ => {} // scalar present where scalar expected — keep it
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp(label: &str) -> PathBuf {
        let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("knogg-migrate-{label}-{n}"))
    }

    /// All fields have `#[serde(default)]` — strict deserialization always succeeds.
    #[derive(Debug, PartialEq, Deserialize, Serialize)]
    struct PermissiveCtx {
        #[serde(default)]
        name: String,
        #[serde(default)]
        count: u32,
        #[serde(default)]
        tags: Vec<String>,
    }

    /// Fields without `#[serde(default)]` — missing fields cause strict failure
    /// and force the fallback path in `read_yaml_typed`.
    #[derive(Debug, PartialEq, Deserialize, Serialize)]
    struct StrictCtx {
        name: String,  // required, no serde(default)
        count: u32,    // required, no serde(default)
        #[serde(default)]
        tags: Vec<String>,
    }

    #[test]
    fn fast_path_returns_valid_doc() {
        let f = tmp("fast");
        fs::write(&f, "name: hello\ncount: 3\ntags: [a, b]\n").unwrap();
        let ctx: PermissiveCtx = read_yaml_typed(&f, &json!({})).unwrap();
        assert_eq!(ctx.name, "hello");
        assert_eq!(ctx.count, 3);
        fs::remove_file(&f).ok();
    }

    #[test]
    fn fallback_injects_missing_required_field() {
        let f = tmp("scalar");
        // `name` is missing → StrictCtx deserialization fails → fallback injects default.
        fs::write(&f, "count: 42\n").unwrap();
        let ctx: StrictCtx = read_yaml_typed(
            &f,
            &json!({"name": "injected", "count": 0, "tags": []}),
        )
        .unwrap();
        // `name` was absent → injected from defaults.
        assert_eq!(ctx.name, "injected");
        // `count` was present in the file → kept.
        assert_eq!(ctx.count, 42);
        fs::remove_file(&f).ok();
    }

    #[test]
    fn fallback_writes_patched_file_to_disk() {
        let f = tmp("write");
        // Missing `name` → fallback runs → patches file with the default `name`.
        fs::write(&f, "count: 7\n").unwrap();

        let _: StrictCtx =
            read_yaml_typed(&f, &json!({"name": "written", "count": 0, "tags": []})).unwrap();

        let on_disk = fs::read_to_string(&f).unwrap();
        // The patched YAML must contain the injected field.
        assert!(on_disk.contains("name") && on_disk.contains("written"),
            "patched file should contain injected name field: {on_disk}");
        fs::remove_file(&f).ok();
    }

    #[test]
    fn existing_value_wins_over_default() {
        let f = tmp("wins");
        fs::write(&f, "name: existing\ncount: 5\ntags: []\n").unwrap();
        let ctx: PermissiveCtx =
            read_yaml_typed(&f, &json!({"name": "OVERWRITE", "count": 99})).unwrap();
        assert_eq!(ctx.name, "existing");
        assert_eq!(ctx.count, 5);
        fs::remove_file(&f).ok();
    }

    #[test]
    fn inject_defaults_merges_nested_objects() {
        let mut target = json!({"a": {"x": 1}});
        let defaults = json!({"a": {"y": 2}, "b": 3});
        inject_defaults(&mut target, &defaults);
        assert_eq!(target["a"]["x"], 1); // preserved
        assert_eq!(target["a"]["y"], 2); // injected
        assert_eq!(target["b"], 3);      // injected top-level
    }

    #[test]
    fn inject_defaults_does_not_overwrite_existing_scalars() {
        let mut target = json!({"x": "keep"});
        inject_defaults(&mut target, &json!({"x": "replace"}));
        assert_eq!(target["x"], "keep");
    }
}
