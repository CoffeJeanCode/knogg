//! Hardened IO layer for the vault: a global write lock and atomic writes.
//!
//! All vault/output writes go through [`atomic_write`] while holding a
//! [`VaultLock`], so concurrent CLI / MCP / watch processes cannot corrupt or
//! partially write files.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};

/// Wait up to 15s for a held lock.
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(15);

/// Polling interval while waiting.
const LOCK_RETRY: Duration = Duration::from_millis(50);

/// Stale-lock threshold: after this age, locks held by dead PIDs are reclaimed.
const STALE_AGE: Duration = Duration::from_secs(30);

/// Global vault lock file (kept for backward compatibility).
const LOCK_FILE: &str = ".lock";

/// JSON metadata persisted inside a lock file.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct LockMeta {
    pid: u32,
    owner: String,
    timestamp: u64,
    intent: String,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Best-effort liveness probe: signal 0 to `pid`. Linux/macOS only; on Windows
/// assume the PID is alive (no recovery on that platform).
#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // SAFETY: kill(pid, 0) only checks for existence; no signal is delivered.
    unsafe { libc::kill(pid as i32, 0) == 0 || *libc::__errno_location() != libc::ESRCH }
}
#[cfg(not(unix))]
fn pid_alive(_pid: u32) -> bool {
    true
}

fn write_meta(path: &Path, intent: &str) -> Result<()> {
    let meta = LockMeta {
        pid: std::process::id(),
        owner: std::env::var("KNOGG_AGENT").unwrap_or_else(|_| "knogg".into()),
        timestamp: unix_now(),
        intent: intent.to_string(),
    };
    let bytes = serde_json::to_vec_pretty(&meta)?;
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(&bytes)?;
    Ok(())
}

fn read_meta(path: &Path) -> Option<LockMeta> {
    let s = fs::read_to_string(path).ok()?;
    serde_json::from_str(&s).ok()
}

/// Best-effort: reclaim a lock file if its owner is dead OR it's stale.
/// Returns true if the lock was removed (caller can retry create_new).
fn try_reclaim(path: &Path) -> bool {
    let Some(meta) = read_meta(path) else { return false; };
    let age = unix_now().saturating_sub(meta.timestamp);
    if age > STALE_AGE.as_secs() && !pid_alive(meta.pid) {
        eprintln!(
            "[lock] reclaiming stale lock {} (pid={} dead, age={}s)",
            path.display(), meta.pid, age
        );
        let _ = fs::remove_file(path);
        return true;
    }
    false
}

fn acquire_path(lock_path: &Path, timeout: Duration, intent: &str) -> Result<()> {
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating lock dir {}", parent.display()))?;
    }
    let deadline = Instant::now() + timeout;
    let mut reclaimed = false;
    loop {
        match write_meta(lock_path, intent) {
            Ok(()) => return Ok(()),
            Err(e) => {
                let is_exists = e.downcast_ref::<std::io::Error>()
                    .map(|ioe| ioe.kind() == std::io::ErrorKind::AlreadyExists)
                    .unwrap_or(false);
                if !is_exists {
                    return Err(anyhow!("acquiring lock {}: {e}", lock_path.display()));
                }
                if !reclaimed && try_reclaim(lock_path) {
                    reclaimed = true;
                    continue;
                }
                if Instant::now() >= deadline {
                    let meta = read_meta(lock_path);
                    bail!(
                        "lock held: {} ({:?}); use `knogg unlock --file <path>` if owner is dead",
                        lock_path.display(), meta
                    );
                }
                sleep(LOCK_RETRY);
            }
        }
    }
}

/// RAII guard for the global vault lock.
pub struct VaultLock {
    lock_path: PathBuf,
}

impl VaultLock {
    pub fn acquire(vault_root: &Path) -> Result<VaultLock> {
        Self::acquire_with_timeout(vault_root, LOCK_TIMEOUT)
    }

    pub fn acquire_with_timeout(vault_root: &Path, timeout: Duration) -> Result<VaultLock> {
        fs::create_dir_all(vault_root)
            .with_context(|| format!("creating vault root {}", vault_root.display()))?;
        let lock_path = vault_root.join(LOCK_FILE);
        acquire_path(&lock_path, timeout, "vault-global")?;
        Ok(VaultLock { lock_path })
    }
}

impl Drop for VaultLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock_path);
    }
}

/// Per-file granular lock: `<file>.lock` next to the target.
#[allow(dead_code)]
pub struct FileLock {
    lock_path: PathBuf,
}

impl FileLock {
    /// Acquire a granular lock for `target`. `intent` is recorded in metadata.
    #[allow(dead_code)]
    pub fn acquire(target: &Path, intent: &str) -> Result<FileLock> {
        let lock_path = lock_path_for(target);
        acquire_path(&lock_path, LOCK_TIMEOUT, intent)?;
        Ok(FileLock { lock_path })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock_path);
    }
}

pub fn lock_path_for(target: &Path) -> PathBuf {
    let mut p = target.as_os_str().to_owned();
    p.push(".lock");
    PathBuf::from(p)
}

/// Atomically write `content` to `path`.
///
/// Writes to a temp file in the *same directory* (so the final rename never
/// crosses a filesystem boundary), then renames it over the destination. On
/// failure the temp file is removed and the destination is left untouched.
/// Parent directories are created as needed.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&parent)
        .with_context(|| format!("creating parent dir {}", parent.display()))?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("invalid destination path: {}", path.display()))?;
    let uniq = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = parent.join(format!(
        ".{file_name}.{}.{uniq}.tmp",
        std::process::id()
    ));

    // Scope the file handle so it is closed before the rename.
    let write_result = (|| -> Result<()> {
        let mut f = File::create(&tmp)
            .with_context(|| format!("creating temp file {}", tmp.display()))?;
        f.write_all(content)
            .with_context(|| format!("writing temp file {}", tmp.display()))?;
        f.sync_all()
            .with_context(|| format!("flushing temp file {}", tmp.display()))?;
        Ok(())
    })();

    if let Err(e) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(anyhow!(
            "renaming {} -> {}: {e}",
            tmp.display(),
            path.display()
        ));
    }
    Ok(())
}

/// Convert days-since-epoch into `(year, month, day)`.
/// Howard Hinnant's `civil_from_days` algorithm.
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// UTC timestamp `YYYYMMDD-HHMMSS`, used to group one run's backups.
pub fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let tod = secs.rem_euclid(86_400);
    let (h, min, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    format!("{y:04}{m:02}{d:02}-{h:02}{min:02}{s:02}")
}

/// Current UTC date as `YYYY-MM-DD`, computed without external crates.
pub fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Back up an existing file's bytes into `<vault_root>/backups/<stamp>/<rel>`.
pub fn backup_file(vault_root: &Path, rel: &Path, current: &[u8], stamp: &str) -> Result<()> {
    let dest = vault_root.join("backups").join(stamp).join(rel);
    atomic_write(&dest, current)
        .with_context(|| format!("backing up to {}", dest.display()))?;
    println!("backup {}", dest.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("vaultio-{label}-{nanos}"))
    }

    #[test]
    fn atomic_write_creates_and_overwrites() {
        let dir = temp_dir("write");
        let target = dir.join("nested/file.yml");

        atomic_write(&target, b"first").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "first");

        atomic_write(&target, b"second").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "second");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn atomic_write_leaves_no_temp_file() {
        let dir = temp_dir("notemp");
        let target = dir.join("file.yml");
        atomic_write(&target, b"data").unwrap();

        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lock_is_exclusive_and_times_out() {
        let root = temp_dir("lock");
        let held = VaultLock::acquire(&root).unwrap();

        // A second acquisition cannot succeed while the first is held.
        let err = VaultLock::acquire_with_timeout(&root, Duration::from_millis(200))
            .err()
            .expect("second lock must fail");
        assert!(err.to_string().contains("lock held"));

        drop(held);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
    }

    #[test]
    fn backup_file_writes_under_backups_dir() {
        let root = temp_dir("backup");
        backup_file(&root, Path::new("state/x.yml"), b"old", "20260515-120000").unwrap();
        let dest = root.join("backups/20260515-120000/state/x.yml");
        assert_eq!(fs::read_to_string(&dest).unwrap(), "old");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn lock_is_released_on_drop() {
        let root = temp_dir("release");
        {
            let _lock = VaultLock::acquire(&root).unwrap();
        }
        // Previous guard dropped -> lock free again.
        let _lock = VaultLock::acquire_with_timeout(&root, Duration::from_millis(200)).unwrap();
        fs::remove_dir_all(&root).ok();
    }
}
