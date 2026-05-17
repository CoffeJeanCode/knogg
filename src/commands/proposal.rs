//! `knogg proposal` — staged state-change proposals for agents.
//!
//! MCP `propose_state_update` writes a *pending* proposal here instead of
//! mutating `active_context.yml` directly. Proposals are applied or rejected
//! explicitly via the CLI.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::core::vault::{apply_patch, audit_patch, resolve_path, safe_vault_path};
use crate::core::vaultio::{atomic_write, today, VaultLock};

const PENDING: &str = "pending";
const APPLIED: &str = "applied";
const REJECTED: &str = "rejected";

#[derive(Debug, Deserialize, Serialize)]
pub struct Proposal {
    pub id: String,
    pub status: String,
    pub target: String,
    pub reason: String,
    pub created: String,
    pub patch: serde_yaml::Value,
}

/// Directory holding proposal files.
fn proposals_dir(root: &Path) -> PathBuf {
    root.join("state/proposals")
}

/// Validate a proposal id: `PROP-` followed by digits only.
fn validate_id(id: &str) -> Result<()> {
    let ok = id
        .strip_prefix("PROP-")
        .map(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        .unwrap_or(false);
    if !ok {
        bail!("invalid proposal id '{id}' (expected PROP-NNNN)");
    }
    Ok(())
}

fn proposal_path(root: &Path, id: &str) -> Result<PathBuf> {
    validate_id(id)?;
    Ok(proposals_dir(root).join(format!("{id}.yml")))
}

/// Next incremental id, `PROP-NNNN`.
fn next_id(root: &Path) -> Result<String> {
    let dir = proposals_dir(root);
    let mut highest = 0u32;
    if dir.is_dir() {
        for entry in fs::read_dir(&dir)? {
            let name = entry?.file_name();
            let name = name.to_string_lossy();
            if let Some(stem) = name.strip_suffix(".yml") {
                if let Some(n) = stem.strip_prefix("PROP-").and_then(|n| n.parse::<u32>().ok()) {
                    highest = highest.max(n);
                }
            }
        }
    }
    Ok(format!("PROP-{:04}", highest + 1))
}

fn write_proposal(root: &Path, prop: &Proposal) -> Result<()> {
    let path = proposal_path(root, &prop.id)?;
    let out = serde_yaml::to_string(prop).map_err(|e| anyhow!("serializing proposal: {e}"))?;
    atomic_write(&path, out.as_bytes())
}

/// Load a proposal by id.
pub fn load(root: &Path, id: &str) -> Result<Proposal> {
    let path = proposal_path(root, id)?;
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("reading proposal {id} at {}", path.display()))?;
    serde_yaml::from_str(&raw).map_err(|e| anyhow!("parsing proposal {id}: {e}"))
}

/// All proposals, sorted by id.
pub fn all(root: &Path) -> Result<Vec<Proposal>> {
    let dir = proposals_dir(root);
    let mut out = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(&dir)? {
            let name = entry?.file_name();
            let name = name.to_string_lossy();
            if let Some(id) = name.strip_suffix(".yml") {
                if validate_id(id).is_ok() {
                    out.push(load(root, id)?);
                }
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Create a new pending proposal; returns its id.
pub fn create(root: &Path, target: &str, patch_json: &JsonValue, reason: &str) -> Result<String> {
    // Reject targets outside the vault.
    safe_vault_path(root, target)?;
    let _lock = VaultLock::acquire(root)?;
    let id = next_id(root)?;
    let patch =
        serde_yaml::to_value(patch_json).map_err(|e| anyhow!("converting patch: {e}"))?;
    let prop = Proposal {
        id: id.clone(),
        status: PENDING.to_string(),
        target: target.to_string(),
        reason: reason.to_string(),
        created: today(),
        patch,
    };
    write_proposal(root, &prop)?;
    Ok(id)
}

/// Apply a pending proposal: re-audit, apply the patch, mark it applied.
pub fn apply(root: &Path, id: &str) -> Result<()> {
    // One lock covers the patch apply and the proposal status update.
    let _lock = VaultLock::acquire(root)?;
    let mut prop = load(root, id)?;
    if prop.status != PENDING {
        bail!(
            "proposal {id} is '{}' — only pending proposals can be applied",
            prop.status
        );
    }
    let patch_json: JsonValue =
        serde_json::to_value(&prop.patch).map_err(|e| anyhow!("converting patch: {e}"))?;
    audit_patch(&patch_json)?;
    apply_patch(root, &prop.target, &patch_json)?;
    prop.status = APPLIED.to_string();
    write_proposal(root, &prop)?;
    Ok(())
}

/// Reject a pending proposal.
pub fn reject(root: &Path, id: &str) -> Result<()> {
    let _lock = VaultLock::acquire(root)?;
    let mut prop = load(root, id)?;
    if prop.status != PENDING {
        bail!(
            "proposal {id} is '{}' — only pending proposals can be rejected",
            prop.status
        );
    }
    prop.status = REJECTED.to_string();
    write_proposal(root, &prop)?;
    Ok(())
}

// ---- CLI wrappers (print human-readable output) ----

pub fn cmd_list(path: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let proposals = all(&root)?;
    if proposals.is_empty() {
        println!("no proposals");
        return Ok(());
    }
    for p in proposals {
        println!("{}  {:9}  {}", p.id, p.status, p.target);
    }
    Ok(())
}

pub fn cmd_show(path: &str, id: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let p = load(&root, id)?;
    println!("id:      {}", p.id);
    println!("status:  {}", p.status);
    println!("target:  {}", p.target);
    println!("created: {}", p.created);
    println!("reason:  {}", p.reason);
    let patch = serde_yaml::to_string(&p.patch).unwrap_or_default();
    println!("patch:\n{}", patch.trim_end());
    Ok(())
}

pub fn cmd_apply(path: &str, id: &str) -> Result<()> {
    let root = resolve_path(path)?;
    apply(&root, id)?;
    println!("applied {id}");
    // F6: after_proposal_apply hooks (lock already released by `apply`).
    if let Err(e) = crate::commands::hooks::run(&root, "after_proposal_apply") {
        eprintln!("hook warning: {e}");
    }
    Ok(())
}

pub fn cmd_reject(path: &str, id: &str) -> Result<()> {
    let root = resolve_path(path)?;
    reject(&root, id)?;
    println!("rejected {id}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::vault::init;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("vault-proposal-{label}-{nanos}"))
    }

    #[test]
    fn create_uses_incremental_ids_and_stays_pending() {
        let root = temp_root("create");
        init(root.to_str().unwrap(), false).unwrap();

        let id1 = create(
            &root,
            "state/active_context.yml",
            &json!({"focus": {"status": "blocked"}}),
            "r1",
        )
        .unwrap();
        let id2 = create(
            &root,
            "state/active_context.yml",
            &json!({"focus": {"status": "done"}}),
            "r2",
        )
        .unwrap();
        assert_eq!(id1, "PROP-0001");
        assert_eq!(id2, "PROP-0002");
        assert_eq!(load(&root, &id1).unwrap().status, "pending");
        assert_eq!(all(&root).unwrap().len(), 2);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn apply_mutates_state_and_blocks_reapply() {
        let root = temp_root("apply");
        init(root.to_str().unwrap(), false).unwrap();
        let id = create(
            &root,
            "state/active_context.yml",
            &json!({"focus": {"status": "blocked"}}),
            "move",
        )
        .unwrap();

        apply(&root, &id).unwrap();
        let ctx = crate::core::vault::read_active_context(&root).unwrap();
        assert_eq!(ctx.focus.status, "blocked");
        assert_eq!(load(&root, &id).unwrap().status, "applied");

        // Already applied -> cannot apply or reject again.
        assert!(apply(&root, &id).is_err());
        assert!(reject(&root, &id).is_err());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn reject_marks_rejected_and_blocks_apply() {
        let root = temp_root("reject");
        init(root.to_str().unwrap(), false).unwrap();
        let id = create(
            &root,
            "state/active_context.yml",
            &json!({"focus": {"status": "done"}}),
            "nope",
        )
        .unwrap();

        reject(&root, &id).unwrap();
        assert_eq!(load(&root, &id).unwrap().status, "rejected");
        assert!(apply(&root, &id).is_err());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn invalid_ids_are_rejected() {
        assert!(validate_id("PROP-0001").is_ok());
        assert!(validate_id("../etc").is_err());
        assert!(validate_id("PROP-").is_err());
        assert!(validate_id("ADR-0001").is_err());
    }
}
