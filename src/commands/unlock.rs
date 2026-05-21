//! `knogg unlock` — Stage 13. Manually clear granular and global lock files.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, Result};

use crate::core::vault::resolve_path;

/// `knogg unlock --file <path>` — remove a single lock file.
pub fn unlock_file(path: &str, file: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let target = root.join(file);
    let lock = crate::core::vaultio::lock_path_for(&target);
    if !lock.exists() {
        return Err(anyhow!("no lock at {}", lock.display()));
    }
    fs::remove_file(&lock)?;
    println!("removed {}", lock.display());
    Ok(())
}

/// `knogg unlock --all` — sweep every `.lock` file under the vault root.
pub fn unlock_all(path: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let mut removed = 0usize;
    walk_locks(&root, &mut |p| {
        if let Err(e) = fs::remove_file(p) {
            eprintln!("failed to remove {}: {e}", p.display());
        } else {
            println!("removed {}", p.display());
            removed += 1;
        }
    })?;
    println!("{removed} lock(s) cleared");
    Ok(())
}

fn walk_locks<F: FnMut(&Path)>(dir: &Path, cb: &mut F) -> Result<()> {
    if !dir.is_dir() { return Ok(()); }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_locks(&path, cb)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("lock")
            || path.file_name().and_then(|s| s.to_str()) == Some(".lock")
        {
            cb(&path);
        }
    }
    Ok(())
}
