//! Transparent schema auto-migrations — Stage 14.
//!
//! Vault YAML files carry a top-level `version` field. On read, if the
//! on-disk version is older than [`CURRENT_VERSION`], a migration ladder
//! upgrades the document in memory and rewrites it to disk atomically.
//! Agents/CLI callers are unaware.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_yaml::Value;

use crate::core::vaultio::atomic_write;

/// The schema version this binary expects for all vault YAMLs.
pub const CURRENT_VERSION: u64 = 1;

/// Read a YAML file and, if older than [`CURRENT_VERSION`], migrate + overwrite.
/// Returns the up-to-date raw string.
pub fn read_and_migrate(file: &Path) -> Result<String> {
    let raw = fs::read_to_string(file)
        .with_context(|| format!("reading {}", file.display()))?;
    let mut doc: Value = match serde_yaml::from_str(&raw) {
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
    if let Value::Mapping(ref mut map) = doc {
        map.insert(Value::String("version".into()), Value::Number(CURRENT_VERSION.into()));
    }
    let out = serde_yaml::to_string(&doc)?;
    atomic_write(file, out.as_bytes())?;
    eprintln!("[migrate] {}: v{} → v{}", file.display(), on_disk, CURRENT_VERSION);
    Ok(out)
}

/// Apply one migration step from `from` → `from+1`. Add new arms as the
/// schema evolves; v0 → v1 is the legacy upgrade that just stamps a version.
fn migrate_step(_doc: &mut Value, _from: u64) -> Result<()> {
    Ok(())
}
