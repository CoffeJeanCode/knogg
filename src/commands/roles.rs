//! `knogg role` — agent role specifications.
//!
//! A role names an agent and states what it is and what it must do, stored in
//! `plans/roles.yml`. Agents fetch their role by name to know their job.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::vault::resolve_path;
use crate::core::vaultio::{atomic_write, VaultLock};

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct RoleSet {
    #[serde(default)]
    pub roles: BTreeMap<String, Role>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Role {
    pub summary: String,
    #[serde(default)]
    pub responsibilities: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
}

fn roles_path(root: &Path) -> std::path::PathBuf {
    root.join("plans/roles.yml")
}

/// Load `plans/roles.yml`; a missing file yields an empty set.
pub fn load(root: &Path) -> Result<RoleSet> {
    match fs::read_to_string(roles_path(root)) {
        Ok(raw) => serde_yaml::from_str(&raw)
            .map_err(|e| anyhow!("parsing roles.yml: {e}")),
        Err(_) => Ok(RoleSet::default()),
    }
}

/// Atomically write the role set. Caller must hold the lock.
fn write(root: &Path, set: &RoleSet) -> Result<()> {
    let out = serde_yaml::to_string(set).map_err(|e| anyhow!("serializing roles.yml: {e}"))?;
    atomic_write(&roles_path(root), out.as_bytes())
}

/// Fetch one role by name.
pub fn get(root: &Path, name: &str) -> Result<Role> {
    load(root)?
        .roles
        .remove(name)
        .ok_or_else(|| anyhow!("unknown role '{name}'"))
}

/// Create or replace a role.
pub fn set_entry(
    root: &Path,
    name: &str,
    summary: &str,
    responsibilities: Vec<String>,
    constraints: Vec<String>,
) -> Result<()> {
    if name.trim().is_empty() {
        bail!("role name is empty");
    }
    let _lock = VaultLock::acquire(root)?;
    let mut set = load(root)?;
    set.roles.insert(
        name.to_string(),
        Role { summary: summary.to_string(), responsibilities, constraints },
    );
    write(root, &set)
}

/// Remove a role by name.
pub fn remove_entry(root: &Path, name: &str) -> Result<()> {
    let _lock = VaultLock::acquire(root)?;
    let mut set = load(root)?;
    if set.roles.remove(name).is_none() {
        bail!("unknown role '{name}'");
    }
    write(root, &set)
}

/// All roles as JSON: `{name, summary}` entries.
pub fn all_json(root: &Path) -> Result<Value> {
    let set = load(root)?;
    let items: Vec<Value> = set
        .roles
        .iter()
        .map(|(n, r)| json!({"name": n, "summary": r.summary}))
        .collect();
    Ok(json!({ "roles": items }))
}

/// One role as JSON, including responsibilities and constraints.
pub fn role_json(root: &Path, name: &str) -> Result<Value> {
    let r = get(root, name)?;
    Ok(json!({
        "name": name,
        "summary": r.summary,
        "responsibilities": r.responsibilities,
        "constraints": r.constraints,
    }))
}

// ---- CLI wrappers ----------------------------------------------------------

pub fn cmd_set(
    path: &str,
    name: &str,
    summary: &str,
    responsibilities: Vec<String>,
    constraints: Vec<String>,
) -> Result<()> {
    let root = resolve_path(path)?;
    set_entry(&root, name, summary, responsibilities, constraints)?;
    println!("role {name} set");
    Ok(())
}

pub fn cmd_list(path: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let set = load(&root)?;
    if set.roles.is_empty() {
        println!("no roles");
        return Ok(());
    }
    for (name, role) in &set.roles {
        println!("{name:14} {}", role.summary);
    }
    Ok(())
}

pub fn cmd_show(path: &str, name: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let role = get(&root, name)?;
    println!("role:    {name}");
    println!("summary: {}", role.summary);
    println!("responsibilities:");
    for r in &role.responsibilities {
        println!("  - {r}");
    }
    println!("constraints:");
    for c in &role.constraints {
        println!("  - {c}");
    }
    Ok(())
}

pub fn cmd_remove(path: &str, name: &str) -> Result<()> {
    let root = resolve_path(path)?;
    remove_entry(&root, name)?;
    println!("role {name} removed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::vault::init;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("knogg-roles-{label}-{nanos}"))
    }

    #[test]
    fn default_roles_parse_and_init_writes_them() {
        let set: RoleSet = serde_yaml::from_str(crate::core::vault::DEFAULT_ROLES).unwrap();
        assert!(set.roles.contains_key("implementer"));

        let root = temp_root("init");
        init(root.to_str().unwrap(), false).unwrap();
        let loaded = load(&root).unwrap();
        assert!(loaded.roles.contains_key("reviewer"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn set_get_remove_roundtrip() {
        let root = temp_root("crud");
        init(root.to_str().unwrap(), false).unwrap();

        set_entry(
            &root,
            "tester",
            "Runs the test suite",
            vec!["Run cargo test".into()],
            vec!["No code changes".into()],
        )
        .unwrap();
        let r = get(&root, "tester").unwrap();
        assert_eq!(r.summary, "Runs the test suite");
        assert_eq!(r.responsibilities.len(), 1);

        remove_entry(&root, "tester").unwrap();
        assert!(get(&root, "tester").is_err());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unknown_role_errors() {
        let root = temp_root("unknown");
        init(root.to_str().unwrap(), false).unwrap();
        assert!(get(&root, "ghost").is_err());
        assert!(remove_entry(&root, "ghost").is_err());
        fs::remove_dir_all(&root).ok();
    }
}
