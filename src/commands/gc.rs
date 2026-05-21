//! `knogg gc` — Stage 15. Reclaim disk space.
//!
//! Rules:
//! - `.knogg/backups/<stamp>/...` older than 7 days → delete the whole stamp dir
//! - `.knogg/state/proposals/<id>.yml` with terminal status (applied|rejected)
//!   older than 2 days → delete
//!
//! Also exposes [`spawn_daemon_gc`], a hourly background task for `knogg watch`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::core::vault::resolve_path;

const BACKUPS_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const PROPOSALS_TTL: Duration = Duration::from_secs(2 * 24 * 3600);

/// `knogg gc`.
pub fn run(path: &str, dry_run: bool) -> Result<()> {
    let root = resolve_path(path)?;
    let report = sweep(&root, dry_run)?;
    println!(
        "{}cleaned {} backup dir(s), {} proposal(s), {} bytes",
        if dry_run { "[dry-run] " } else { "" },
        report.backup_dirs,
        report.proposals,
        report.bytes
    );
    Ok(())
}

/// Spawn a tokio task that runs [`sweep`] every hour. Best-effort.
/// Safe to call from sync code as long as a tokio runtime is current.
pub fn spawn_daemon_gc(root: &Path) {
    let root = root.to_path_buf();
    std::thread::spawn(move || {
        let interval = Duration::from_secs(3600);
        loop {
            std::thread::sleep(interval);
            match sweep(&root, false) {
                Ok(r) => {
                    if r.backup_dirs + r.proposals > 0 {
                        eprintln!(
                            "[gc] swept {} backups, {} proposals, {} bytes",
                            r.backup_dirs, r.proposals, r.bytes
                        );
                    }
                }
                Err(e) => eprintln!("[gc] sweep error: {e}"),
            }
        }
    });
}

#[derive(Default)]
pub struct Report {
    pub backup_dirs: usize,
    pub proposals: usize,
    pub bytes: u64,
}

pub fn sweep(root: &Path, dry_run: bool) -> Result<Report> {
    let mut r = Report::default();
    let now = SystemTime::now();

    let backups = root.join("backups");
    if backups.is_dir() {
        for entry in fs::read_dir(&backups)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() { continue; }
            if older_than(&path, &now, BACKUPS_TTL) {
                let size = dir_size(&path).unwrap_or(0);
                if !dry_run { fs::remove_dir_all(&path)?; }
                r.backup_dirs += 1;
                r.bytes += size;
            }
        }
    }

    let proposals = root.join("state/proposals");
    if proposals.is_dir() {
        for entry in fs::read_dir(&proposals)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() { continue; }
            if path.extension().and_then(|s| s.to_str()) != Some("yml") { continue; }
            if !is_terminal_proposal(&path).unwrap_or(false) { continue; }
            if older_than(&path, &now, PROPOSALS_TTL) {
                let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                if !dry_run { fs::remove_file(&path)?; }
                r.proposals += 1;
                r.bytes += size;
            }
        }
    }

    Ok(r)
}

fn older_than(path: &Path, now: &SystemTime, ttl: Duration) -> bool {
    let Ok(meta) = path.metadata() else { return false; };
    let modified = meta.modified().unwrap_or(UNIX_EPOCH);
    now.duration_since(modified).map(|d| d > ttl).unwrap_or(false)
}

fn dir_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    if path.is_dir() {
        for entry in fs::read_dir(path)
            .with_context(|| format!("reading {}", path.display()))?
        {
            let entry = entry?;
            let p = entry.path();
            if p.is_dir() {
                total += dir_size(&p)?;
            } else if let Ok(m) = p.metadata() {
                total += m.len();
            }
        }
    }
    Ok(total)
}

fn is_terminal_proposal(path: &Path) -> Result<bool> {
    let raw = fs::read_to_string(path)?;
    let v: serde_yaml::Value = serde_yaml::from_str(&raw)?;
    let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
    Ok(matches!(status, "applied" | "rejected"))
}

#[allow(dead_code)]
fn _unused(_: PathBuf) {}
