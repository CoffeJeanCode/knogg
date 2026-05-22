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
const SUPERSEDED: &str = "superseded";

/// Risk tier for proposal auto-apply (ADR-0011).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalRisk {
    Low,
    High,
}

/// Outcome of staging a proposal (may auto-apply or supersede siblings).
#[derive(Debug)]
pub struct CreateOutcome {
    pub proposal_id: String,
    pub status: String,
    pub superseded: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Proposal {
    pub id: String,
    pub status: String,
    pub target: String,
    pub reason: String,
    pub created: String,
    pub patch: serde_yaml::Value,
    /// Vault project name when the proposal was created.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub project: String,
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

/// Classify proposal risk from target and patch keys (ADR-0011).
pub fn classify_risk(target: &str, patch: &JsonValue) -> ProposalRisk {
    const LOW_TARGETS: [&str; 2] = ["state/active_context.yml", "state/brief.yml"];
    if !LOW_TARGETS.contains(&target) {
        return ProposalRisk::High;
    }
    const ALLOWED: [&str; 4] = ["focus", "next_actions", "handoff", "constraints"];
    let Some(obj) = patch.as_object() else {
        return ProposalRisk::Low;
    };
    if obj.keys().any(|k| !ALLOWED.contains(&k.as_str())) {
        ProposalRisk::High
    } else {
        ProposalRisk::Low
    }
}

/// Mark pending proposals for the same target as superseded.
pub fn supersede_pending_same_target(root: &Path, target: &str) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    for mut prop in all(root)? {
        if prop.status == PENDING && prop.target == target {
            prop.status = SUPERSEDED.to_string();
            write_proposal(root, &prop)?;
            ids.push(prop.id);
        }
    }
    Ok(ids)
}

/// Stage a proposal; optionally auto-apply low-risk patches.
pub fn create_with_policy(
    root: &Path,
    target: &str,
    patch_json: &JsonValue,
    reason: &str,
    autoapply_low: bool,
) -> Result<CreateOutcome> {
    safe_vault_path(root, target)?;
    let _lock = VaultLock::acquire(root)?;
    let superseded = supersede_pending_same_target(root, target)?;
    let risk = classify_risk(target, patch_json);
    let id = next_id(root)?;
    let patch =
        serde_yaml::to_value(patch_json).map_err(|e| anyhow!("converting patch: {e}"))?;
    let project = crate::core::vault::read_active_context(root)
        .map(|c| c.project.name)
        .unwrap_or_default();
    let prop = Proposal {
        id: id.clone(),
        status: PENDING.to_string(),
        target: target.to_string(),
        reason: reason.to_string(),
        created: today(),
        patch,
        project,
    };
    write_proposal(root, &prop)?;

    if autoapply_low && risk == ProposalRisk::Low {
        apply_inner(root, &id)?;
        return Ok(CreateOutcome {
            proposal_id: id,
            status: APPLIED.to_string(),
            superseded,
        });
    }
    Ok(CreateOutcome {
        proposal_id: id,
        status: PENDING.to_string(),
        superseded,
    })
}

/// Create a new pending proposal; returns its id (no auto-apply).
#[cfg(test)]
pub fn create(root: &Path, target: &str, patch_json: &JsonValue, reason: &str) -> Result<String> {
    Ok(create_with_policy(root, target, patch_json, reason, false)?.proposal_id)
}

/// Apply one proposal. Caller must hold the vault lock.
pub fn apply_inner(root: &Path, id: &str) -> Result<()> {
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

/// Apply a pending proposal (acquires lock).
#[allow(dead_code)]
pub fn apply(root: &Path, id: &str) -> Result<()> {
    let _lock = VaultLock::acquire(root)?;
    apply_inner(root, id)
}

/// Apply many proposals under one lock. Best-effort — not atomic.
pub fn apply_many(root: &Path, ids: &[String]) -> Result<Vec<(String, Result<()>)>> {
    let _lock = VaultLock::acquire(root)?;
    Ok(ids
        .iter()
        .map(|id| (id.clone(), apply_inner(root, id)))
        .collect())
}

/// Reject one proposal. Caller must hold the vault lock.
pub fn reject_inner(root: &Path, id: &str) -> Result<()> {
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

/// Reject many proposals under one lock. Best-effort — not atomic.
pub fn reject_many(root: &Path, ids: &[String]) -> Result<Vec<(String, Result<()>)>> {
    let _lock = VaultLock::acquire(root)?;
    Ok(ids
        .iter()
        .map(|id| (id.clone(), reject_inner(root, id)))
        .collect())
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

fn print_proposal(p: &Proposal) {
    println!("id:      {}", p.id);
    println!("status:  {}", p.status);
    println!("target:  {}", p.target);
    println!("created: {}", p.created);
    println!("reason:  {}", p.reason);
    if !p.project.is_empty() {
        println!("project: {}", p.project);
    }
    let patch = serde_yaml::to_string(&p.patch).unwrap_or_default();
    println!("patch:\n{}", patch.trim_end());
}

pub fn cmd_show(path: &str, ids: &[String]) -> Result<()> {
    let root = resolve_path(path)?;
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            println!();
        }
        print_proposal(&load(&root, id)?);
    }
    Ok(())
}

fn report_batch_results(results: &[(String, Result<()>)], ok_label: &str) -> (bool, bool) {
    let mut any_ok = false;
    let mut any_fail = false;
    for (id, res) in results {
        match res {
            Ok(()) => {
                println!("{ok_label} {id}");
                any_ok = true;
            }
            Err(e) => {
                println!("FAILED {id}: {e:#}");
                any_fail = true;
            }
        }
    }
    (any_ok, any_fail)
}

pub fn cmd_apply(path: &str, ids: &[String]) -> Result<()> {
    let root = resolve_path(path)?;
    let results = apply_many(&root, ids)?;
    let (any_ok, any_fail) = report_batch_results(&results, "applied");
    if any_ok {
        if let Err(e) = crate::commands::hooks::run(&root, "after_proposal_apply") {
            eprintln!("hook warning: {e}");
        }
    }
    if any_fail {
        bail!("one or more proposals failed to apply");
    }
    Ok(())
}

pub fn cmd_reject(path: &str, ids: &[String]) -> Result<()> {
    let root = resolve_path(path)?;
    let results = reject_many(&root, ids)?;
    let (_, any_fail) = report_batch_results(&results, "rejected");
    if any_fail {
        bail!("one or more proposals failed to reject");
    }
    Ok(())
}

/// Remove terminal proposals from disk.
pub fn gc(
    root: &Path,
    statuses: &[String],
    keep: Option<usize>,
    project: Option<&str>,
) -> Result<usize> {
    let terminal: Vec<&str> = if statuses.is_empty() {
        vec![APPLIED, REJECTED]
    } else {
        statuses.iter().map(String::as_str).collect()
    };
    let _lock = VaultLock::acquire(root)?;
    let mut removed = 0usize;
    let mut by_status: std::collections::BTreeMap<String, Vec<Proposal>> =
        std::collections::BTreeMap::new();
    for p in all(root)? {
        if !terminal.contains(&p.status.as_str()) {
            continue;
        }
        if let Some(proj) = project {
            if !p.project.is_empty() && p.project != proj {
                continue;
            }
        }
        by_status.entry(p.status.clone()).or_default().push(p);
    }
    for proposals in by_status.values_mut() {
        proposals.sort_by(|a, b| a.id.cmp(&b.id));
        let drop_count = match keep {
            Some(n) if proposals.len() > n => proposals.len() - n,
            _ => proposals.len(),
        };
        for p in proposals.iter().take(drop_count) {
            fs::remove_file(proposal_path(root, &p.id)?)?;
            removed += 1;
        }
    }
    Ok(removed)
}

pub fn cmd_gc(
    path: &str,
    statuses: Vec<String>,
    keep: Option<usize>,
    project: Option<String>,
) -> Result<()> {
    let root = resolve_path(path)?;
    for s in &statuses {
        if s != APPLIED && s != REJECTED && s != PENDING {
            bail!("invalid gc status '{s}' (allowed: applied, rejected, pending)");
        }
    }
    let n = gc(&root, &statuses, keep, project.as_deref())?;
    println!("gc removed {n} proposal(s)");
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
        assert_eq!(load(&root, &id1).unwrap().status, SUPERSEDED);
        assert_eq!(load(&root, &id2).unwrap().status, PENDING);
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
        let re = reject_many(&root, &[id.clone()]).unwrap();
        assert!(re[0].1.is_err());
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

        reject_many(&root, &[id.clone()]).unwrap();
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

    #[test]
    fn gc_removes_terminal_proposals() {
        let root = temp_root("gc");
        init(root.to_str().unwrap(), false).unwrap();
        let id = create(
            &root,
            "state/active_context.yml",
            &json!({"focus": {"status": "done"}}),
            "done",
        )
        .unwrap();
        apply(&root, &id).unwrap();
        assert_eq!(all(&root).unwrap().len(), 1);
        let n = gc(&root, &[], None, None).unwrap();
        assert_eq!(n, 1);
        assert!(all(&root).unwrap().is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn apply_many_is_best_effort() {
        let root = temp_root("batch");
        init(root.to_str().unwrap(), false).unwrap();
        let id1 = create(
            &root,
            "state/active_context.yml",
            &json!({"focus": {"status": "blocked"}}),
            "a",
        )
        .unwrap();
        let id2 = create(
            &root,
            "plans/roles.yml",
            &json!({"roles": {"tester": {"summary": "tests", "responsibilities": [], "constraints": []}}}),
            "b",
        )
        .unwrap();
        let results = apply_many(
            &root,
            &[id1.clone(), "PROP-9999".into(), id2.clone()],
        )
        .unwrap();
        assert_eq!(results.len(), 3);
        assert!(results[0].1.is_ok());
        assert!(results[1].1.is_err());
        assert!(results[2].1.is_ok());
        assert_eq!(load(&root, &id1).unwrap().status, APPLIED);
        assert_eq!(load(&root, &id2).unwrap().status, APPLIED);
        let ctx = crate::core::vault::read_active_context(&root).unwrap();
        assert_eq!(ctx.focus.status, "blocked");
        let roles = fs::read_to_string(root.join("plans/roles.yml")).unwrap();
        assert!(roles.contains("tester"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn proposal_records_project_tag() {
        let root = temp_root("proj");
        init(root.to_str().unwrap(), false).unwrap();
        let id = create(
            &root,
            "state/active_context.yml",
            &json!({"focus": {"status": "todo"}}),
            "r",
        )
        .unwrap();
        assert_eq!(load(&root, &id).unwrap().project, "knogg");
        fs::remove_dir_all(&root).ok();
    }
}
