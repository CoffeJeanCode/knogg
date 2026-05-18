//! Canonical compact **brief** at `state/brief.yml`.
//!
//! The brief is a small, token-cheap snapshot of the vault: focus, next
//! actions, constraints, recent decisions, handoff summary. Agents and the
//! MCP server read it instead of loading the whole vault.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::commands::decision::{self, DecisionSummary};
use crate::core::vault::{read_active_context, write_active_context, Focus};
use crate::core::vaultio::{atomic_write, today, VaultLock};

/// Recent decisions kept in the brief (never the full log).
const MAX_DECISIONS: usize = 5;

#[derive(Debug, Deserialize, Serialize)]
pub struct Brief {
    pub generated_at: String,
    pub project: String,
    pub focus: Focus,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<String>,
    #[serde(default)]
    pub recent_decisions: Vec<DecisionSummary>,
    #[serde(default)]
    pub handoff_summary: String,
    /// Fingerprint of active_context + recent decisions; used by ensure_fresh.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_hash: String,
}

fn brief_path(root: &Path) -> PathBuf {
    root.join("state/brief.yml")
}

/// Hash inputs that drive the brief so we can skip redundant refreshes.
pub fn compute_source_hash(root: &Path) -> Result<String> {
    let ctx = read_active_context(root)?;
    let recent = decision::recent_summaries(root, MAX_DECISIONS)?;
    let mut hasher = DefaultHasher::new();
    ctx.project.name.hash(&mut hasher);
    ctx.focus.stage.hash(&mut hasher);
    ctx.focus.task.hash(&mut hasher);
    ctx.focus.status.hash(&mut hasher);
    ctx.constraints.hash(&mut hasher);
    ctx.next_actions.hash(&mut hasher);
    ctx.handoff.summary.hash(&mut hasher);
    for d in &recent {
        d.id.hash(&mut hasher);
        d.title.hash(&mut hasher);
        d.status.hash(&mut hasher);
    }
    Ok(format!("{:016x}", hasher.finish()))
}

/// Build a brief from the current vault state (no write).
pub fn build(root: &Path) -> Result<Brief> {
    let ctx = read_active_context(root)?;
    let recent = decision::recent_summaries(root, MAX_DECISIONS)?;
    let source_hash = compute_source_hash(root)?;
    Ok(Brief {
        generated_at: today(),
        project: ctx.project.name,
        focus: ctx.focus,
        constraints: ctx.constraints,
        next_actions: ctx.next_actions,
        recent_decisions: recent,
        handoff_summary: ctx.handoff.summary,
        source_hash,
    })
}

/// Regenerate and atomically write `state/brief.yml`.
pub fn refresh(root: &Path) -> Result<Brief> {
    let brief = build(root)?;
    let _lock = VaultLock::acquire(root)?;
    let out = serde_yaml::to_string(&brief).map_err(|e| anyhow!("serializing brief: {e}"))?;
    atomic_write(&brief_path(root), out.as_bytes())?;
    Ok(brief)
}

/// Load `state/brief.yml` if present.
pub fn load(root: &Path) -> Result<Option<Brief>> {
    match fs::read_to_string(brief_path(root)) {
        Ok(raw) => Ok(Some(
            serde_yaml::from_str(&raw).map_err(|e| anyhow!("parsing brief.yml: {e}"))?,
        )),
        Err(_) => Ok(None),
    }
}

/// Load the brief, regenerating it if absent.
pub fn load_or_refresh(root: &Path) -> Result<Brief> {
    match load(root)? {
        Some(b) => Ok(b),
        None => refresh(root),
    }
}

/// Refresh only when the source hash differs from the on-disk brief.
pub fn ensure_fresh(root: &Path) -> Result<()> {
    let current = compute_source_hash(root)?;
    if let Some(brief) = load(root)? {
        if brief.source_hash == current {
            return Ok(());
        }
    }
    refresh(root)?;
    Ok(())
}

/// Write `handoff.summary` from current focus and next actions when empty.
pub fn auto_fill_handoff_summary(root: &Path) -> Result<bool> {
    let brief = build(root)?;
    if !brief.handoff_summary.trim().is_empty() {
        return Ok(false);
    }
    let next = if brief.next_actions.is_empty() {
        "(none)".to_string()
    } else {
        brief.next_actions.join("; ")
    };
    let summary = format!(
        "{} — {} ({}). Next: {}",
        brief.focus.stage, brief.focus.task, brief.focus.status, next
    );
    let _lock = VaultLock::acquire(root)?;
    let mut ctx = read_active_context(root)?;
    ctx.handoff.summary = summary;
    write_active_context(root, &ctx)?;
    Ok(true)
}

// ---- CLI wrappers ----------------------------------------------------------

pub fn cmd_refresh(path: &str) -> Result<()> {
    let root = crate::core::vault::resolve_path(path)?;
    refresh(&root)?;
    println!("brief refreshed");
    Ok(())
}

pub fn cmd_show(path: &str) -> Result<()> {
    let root = crate::core::vault::resolve_path(path)?;
    let brief = load_or_refresh(&root)?;
    let yaml = serde_yaml::to_string(&brief).map_err(|e| anyhow!("rendering brief: {e}"))?;
    print!("{yaml}");
    Ok(())
}

pub fn cmd_doctor(path: &str) -> Result<()> {
    let root = crate::core::vault::resolve_path(path)?;
    println!("brief doctor\n");
    match load(&root) {
        Ok(Some(b)) if !b.generated_at.is_empty() => {
            let current = compute_source_hash(&root).unwrap_or_default();
            if !b.source_hash.is_empty() && b.source_hash != current {
                println!("[warn] brief.yml stale (run `knogg brief refresh`)");
            } else {
                println!("[ok] state/brief.yml ({})", b.generated_at);
            }
            println!("\nResult: healthy");
            Ok(())
        }
        Ok(Some(_)) => {
            println!("[error] brief.yml has no generated_at");
            println!("\nResult: unhealthy");
            std::process::exit(1);
        }
        Ok(None) => {
            println!("[error] state/brief.yml missing (run `knogg brief refresh`)");
            println!("\nResult: unhealthy");
            std::process::exit(1);
        }
        Err(e) => {
            println!("[error] {e}");
            println!("\nResult: unhealthy");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::vault::init;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("knogg-brief-{label}-{nanos}"))
    }

    #[test]
    fn refresh_then_load() {
        let root = temp_root("refresh");
        init(root.to_str().unwrap(), false).unwrap();
        let b = refresh(&root).unwrap();
        assert_eq!(b.project, "knogg");
        assert!(!b.source_hash.is_empty());
        assert!(brief_path(&root).is_file());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ensure_fresh_skips_when_unchanged() {
        let root = temp_root("hash");
        init(root.to_str().unwrap(), false).unwrap();
        refresh(&root).unwrap();
        let mtime = fs::metadata(brief_path(&root)).unwrap().modified().unwrap();
        ensure_fresh(&root).unwrap();
        assert_eq!(fs::metadata(brief_path(&root)).unwrap().modified().unwrap(), mtime);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn auto_fill_handoff_summary_when_empty() {
        let root = temp_root("fill");
        init(root.to_str().unwrap(), false).unwrap();
        assert!(auto_fill_handoff_summary(&root).unwrap());
        let ctx = read_active_context(&root).unwrap();
        assert!(ctx.handoff.summary.contains("Stage"));
        fs::remove_dir_all(&root).ok();
    }
}
