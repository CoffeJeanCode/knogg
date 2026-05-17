use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::Duration;

use anyhow::{bail, Result};
use notify::{recommended_watcher, RecursiveMode, Watcher};

use crate::commands::sync::sync;
use crate::core::vault::resolve_path;

/// Quiet period before a burst of events triggers a sync (within 300-500ms).
const DEBOUNCE: Duration = Duration::from_millis(400);

/// File watched for state changes.
const TARGET: &str = "active_context.yml";

/// True if any of the changed paths is the watched state file.
fn touches_target(paths: &[PathBuf], target: &Path) -> bool {
    paths.iter().any(|p| {
        p == target || p.file_name() == target.file_name()
    })
}

/// `knogg watch`: re-run sync whenever `state/active_context.yml` changes.
///
/// `marker` is the generated-by marker (from `knogg.toml`, or the default).
pub fn watch(path: &str, marker: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let state_dir = root.join("state");
    let target = state_dir.join(TARGET);

    if !target.exists() {
        bail!(
            "{} not found (run `knogg init` first?)",
            target.display()
        );
    }

    let (tx, rx) = channel();
    let mut watcher = recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;
    // Watch the state/ dir only — never core/ — to observe atomic file replaces.
    watcher.watch(&state_dir, RecursiveMode::NonRecursive)?;

    println!("watching {} for changes (Ctrl-C to stop)", target.display());

    loop {
        // Block until the first event of a burst.
        let first = match rx.recv() {
            Ok(ev) => ev,
            Err(_) => break, // watcher dropped
        };

        let mut relevant = matches!(&first, Ok(ev) if touches_target(&ev.paths, &target));

        // Debounce: keep draining until the channel is quiet for DEBOUNCE.
        loop {
            match rx.recv_timeout(DEBOUNCE) {
                Ok(ev) => {
                    if let Ok(ev) = &ev {
                        relevant |= touches_target(&ev.paths, &target);
                    }
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }

        if relevant {
            println!("change detected in {TARGET}, syncing...");
            // sync writes tool configs outside state/, so this cannot loop.
            if let Err(e) = sync(path, false, marker, false) {
                eprintln!("sync failed: {e}");
            }
        }
    }

    Ok(())
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
        std::env::temp_dir().join(format!("vault-watch-{label}-{nanos}"))
    }

    #[test]
    fn debounce_is_within_spec() {
        assert!(DEBOUNCE >= Duration::from_millis(300));
        assert!(DEBOUNCE <= Duration::from_millis(500));
    }

    #[test]
    fn touches_target_matches_state_file_only() {
        let target = PathBuf::from("/v/state/active_context.yml");
        assert!(touches_target(
            &[PathBuf::from("/v/state/active_context.yml")],
            &target
        ));
        // Atomic editors may emit a tmp path; the real file in the burst still matches.
        assert!(touches_target(
            &[
                PathBuf::from("/v/state/active_context.yml.tmp"),
                PathBuf::from("/v/state/active_context.yml"),
            ],
            &target
        ));
        assert!(!touches_target(
            &[PathBuf::from("/v/state/decision_log.yml")],
            &target
        ));
    }

    #[test]
    fn watch_errors_when_state_file_missing() {
        let root = temp_root("missing");
        // No init -> no state/active_context.yml.
        assert!(watch(root.to_str().unwrap(), crate::core::vault::MARKER).is_err());
    }

    #[test]
    fn watch_rejects_path_traversal() {
        assert!(watch("../escape/.knogg", crate::core::vault::MARKER).is_err());
    }

    #[test]
    fn init_then_target_exists() {
        let root = temp_root("exists");
        init(root.to_str().unwrap(), false).unwrap();
        assert!(root.join("state/active_context.yml").exists());
        std::fs::remove_dir_all(&root).ok();
    }
}
