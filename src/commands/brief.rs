//! Canonical compact **brief** at `state/brief.yml`.
//!
//! The brief is a small, token-cheap snapshot of the vault: focus, next
//! actions, constraints, recent decisions, handoff summary. Agents and the
//! MCP server read it instead of loading the whole vault.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::commands::decision::{self, DecisionSummary};
use crate::core::vault::{read_active_context, Focus};
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
}

fn brief_path(root: &Path) -> PathBuf {
    root.join("state/brief.yml")
}

/// Build a brief from the current vault state (no write).
pub fn build(root: &Path) -> Result<Brief> {
    let ctx = read_active_context(root)?;
    let recent = decision::recent_summaries(root, MAX_DECISIONS)?;
    Ok(Brief {
        generated_at: today(),
        project: ctx.project.name,
        focus: ctx.focus,
        constraints: ctx.constraints,
        next_actions: ctx.next_actions,
        recent_decisions: recent,
        handoff_summary: ctx.handoff.summary,
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

/// Regenerate the brief only if it is missing.
pub fn ensure_fresh(root: &Path) -> Result<()> {
    if load(root)?.is_none() {
        refresh(root)?;
    }
    Ok(())
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
            println!("[ok] state/brief.yml ({})", b.generated_at);
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
        assert!(brief_path(&root).is_file());
        assert!(load(&root).unwrap().is_some());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn load_or_refresh_regenerates_when_missing() {
        let root = temp_root("missing");
        init(root.to_str().unwrap(), false).unwrap();
        assert!(load(&root).unwrap().is_none());
        let b = load_or_refresh(&root).unwrap();
        assert!(!b.generated_at.is_empty());
        assert!(brief_path(&root).is_file());
        fs::remove_dir_all(&root).ok();
    }
}
