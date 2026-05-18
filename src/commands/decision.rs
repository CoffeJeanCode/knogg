//! `knogg decision` — append ADR entries to `state/decision_log.yml`.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::core::vault::resolve_path;
use crate::core::vaultio::{atomic_write, VaultLock};

/// Valid decision statuses.
pub const ALLOWED_DECISION_STATUS: [&str; 4] =
    ["proposed", "accepted", "rejected", "superseded"];

#[derive(Debug, Default, Deserialize, Serialize)]
struct DecisionLog {
    #[serde(default)]
    decisions: Vec<Decision>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Decision {
    id: String,
    date: String,
    title: String,
    status: String,
    scope: String,
    reason: String,
}

/// Append a new ADR entry; returns its id. Caller need not hold the lock.
pub fn add_entry(
    root: &Path,
    title: &str,
    reason: &str,
    status: &str,
    scope: &str,
) -> Result<String> {
    if !ALLOWED_DECISION_STATUS.contains(&status) {
        bail!(
            "invalid decision status '{status}' (allowed: {})",
            ALLOWED_DECISION_STATUS.join(", ")
        );
    }

    let _lock = VaultLock::acquire(root)?;
    let file = root.join("state/decision_log.yml");
    let raw = fs::read_to_string(&file)
        .with_context(|| format!("reading {} (run `knogg init`?)", file.display()))?;
    let mut log: DecisionLog = serde_yaml::from_str(&raw)
        .map_err(|e| anyhow!("parsing {}: {e}", file.display()))?;

    let id = next_id(&log);
    log.decisions.push(Decision {
        id: id.clone(),
        date: crate::core::vaultio::today(),
        title: title.to_string(),
        status: status.to_string(),
        scope: scope.to_string(),
        reason: reason.to_string(),
    });

    let out = serde_yaml::to_string(&log)
        .map_err(|e| anyhow!("serializing decision log: {e}"))?;
    atomic_write(&file, out.as_bytes())?;
    Ok(id)
}

/// `knogg decision add`: append a new ADR entry (CLI wrapper).
pub fn add(path: &str, title: &str, reason: &str, status: &str, scope: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let id = add_entry(&root, title, reason, status, scope)?;
    println!("decision added: {id}");
    Ok(())
}

fn validate_status(status: &str) -> Result<()> {
    if ALLOWED_DECISION_STATUS.contains(&status) {
        Ok(())
    } else {
        bail!(
            "invalid decision status '{status}' (allowed: {})",
            ALLOWED_DECISION_STATUS.join(", ")
        )
    }
}

/// Update one ADR status. Caller must hold the vault lock.
fn set_status_inner(log: &mut DecisionLog, id: &str, status: &str) -> Result<()> {
    validate_status(status)?;
    let d = log
        .decisions
        .iter_mut()
        .find(|d| d.id == id)
        .ok_or_else(|| anyhow!("decision '{id}' not found"))?;
    d.status = status.to_string();
    Ok(())
}

/// Update many ADR statuses under one lock. Best-effort — not atomic across ids.
pub fn set_status_many(
    root: &Path,
    ids: &[String],
    status: &str,
) -> Result<Vec<(String, Result<()>)>> {
    validate_status(status)?;
    let _lock = VaultLock::acquire(root)?;
    let file = root.join("state/decision_log.yml");
    let raw = fs::read_to_string(&file)
        .with_context(|| format!("reading {} (run `knogg init`?)", file.display()))?;
    let mut log: DecisionLog = serde_yaml::from_str(&raw)
        .map_err(|e| anyhow!("parsing {}: {e}", file.display()))?;

    let results: Vec<_> = ids
        .iter()
        .map(|id| (id.clone(), set_status_inner(&mut log, id, status)))
        .collect();

    let out = serde_yaml::to_string(&log)
        .map_err(|e| anyhow!("serializing decision log: {e}"))?;
    atomic_write(&file, out.as_bytes())?;
    Ok(results)
}

/// `knogg decision set-status`: update ADR status(es).
pub fn cmd_set_status(path: &str, ids: &[String], status: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let results = set_status_many(&root, ids, status)?;
    let mut any_fail = false;
    for (id, res) in &results {
        match res {
            Ok(()) => println!("updated {id} -> {status}"),
            Err(e) => {
                println!("FAILED {id}: {e:#}");
                any_fail = true;
            }
        }
    }
    if any_fail {
        bail!("one or more decisions failed to update");
    }
    Ok(())
}

/// Compact decision summary for the brief.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DecisionSummary {
    pub id: String,
    pub title: String,
    pub status: String,
}

/// The most recent decisions as compact summaries (newest last).
pub fn recent_summaries(root: &Path, limit: usize) -> Result<Vec<DecisionSummary>> {
    let file = root.join("state/decision_log.yml");
    let raw = match fs::read_to_string(&file) {
        Ok(r) => r,
        Err(_) => return Ok(Vec::new()),
    };
    let log: DecisionLog =
        serde_yaml::from_str(&raw).map_err(|e| anyhow!("parsing decision_log.yml: {e}"))?;
    let start = log.decisions.len().saturating_sub(limit);
    Ok(log.decisions[start..]
        .iter()
        .map(|d| DecisionSummary {
            id: d.id.clone(),
            title: d.title.clone(),
            status: d.status.clone(),
        })
        .collect())
}

/// Next incremental id: `ADR-NNNN`, one past the highest existing number.
fn next_id(log: &DecisionLog) -> String {
    let highest = log
        .decisions
        .iter()
        .filter_map(|d| d.id.strip_prefix("ADR-"))
        .filter_map(|n| n.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("ADR-{:04}", highest + 1)
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
        std::env::temp_dir().join(format!("vault-decision-{label}-{nanos}"))
    }

    #[test]
    fn add_creates_incremental_ids() {
        let root = temp_root("ids");
        let p = root.to_str().unwrap();
        init(p, false).unwrap();

        add(p, "First", "because", "accepted", "global").unwrap();
        add(p, "Second", "also", "proposed", "local").unwrap();

        let raw = fs::read_to_string(root.join("state/decision_log.yml")).unwrap();
        let log: DecisionLog = serde_yaml::from_str(&raw).unwrap();
        assert_eq!(log.decisions.len(), 2);
        assert_eq!(log.decisions[0].id, "ADR-0001");
        assert_eq!(log.decisions[1].id, "ADR-0002");
        assert_eq!(log.decisions[1].status, "proposed");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn add_rejects_invalid_status() {
        let root = temp_root("badstatus");
        let p = root.to_str().unwrap();
        init(p, false).unwrap();
        assert!(add(p, "X", "y", "maybe", "global").is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn set_status_updates_and_is_best_effort() {
        let root = temp_root("setstatus");
        let p = root.to_str().unwrap();
        init(p, false).unwrap();
        add(p, "A", "r", "proposed", "global").unwrap();
        add(p, "B", "r", "proposed", "global").unwrap();

        let results = set_status_many(
            &root,
            &["ADR-0001".into(), "ADR-9999".into(), "ADR-0002".into()],
            "accepted",
        )
        .unwrap();
        assert!(results[0].1.is_ok());
        assert!(results[1].1.is_err());
        assert!(results[2].1.is_ok());

        let raw = fs::read_to_string(root.join("state/decision_log.yml")).unwrap();
        let log: DecisionLog = serde_yaml::from_str(&raw).unwrap();
        assert_eq!(log.decisions[0].status, "accepted");
        assert_eq!(log.decisions[1].status, "accepted");
        std::fs::remove_dir_all(&root).ok();
    }
}
