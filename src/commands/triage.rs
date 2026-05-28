//! `knogg triage` — interactive human review of pending proposals.

use std::io::{self, Write};
use std::path::Path;

use anyhow::Result;

use crate::commands::proposal::{all, apply_inner, load, Proposal};
use crate::core::vault::resolve_path;
use crate::core::vaultio::VaultLock;

const PENDING: &str = "pending";
const REJECTED: &str = "rejected";
const APPLIED: &str = "applied";

/// Return proposals that are not yet in a terminal state (applied / rejected).
fn pending_proposals(root: &Path) -> Result<Vec<Proposal>> {
    Ok(all(root)?
        .into_iter()
        .filter(|p| p.status != APPLIED && p.status != REJECTED)
        .collect())
}

/// Print a proposal for human review.
fn print_proposal_summary(p: &Proposal) {
    println!("\n{}", "─".repeat(60));
    println!("Proposal : {}", p.id);
    println!("Status   : {}", p.status);
    println!("Target   : {}", p.target);
    if !p.reason.is_empty() {
        println!("Reason   : {}", p.reason);
    }

    let patch = serde_yaml::to_string(&p.patch).unwrap_or_default();
    println!("Patch    :\n{}", indent(patch.trim_end(), "  "));

    if let Some(adr) = &p.adr_proposal {
        println!("ADR      : {} — {}", adr.title, adr.reason);
    }
    if let Some(msg) = &p.message_to_human {
        println!("Message  : {msg}");
    }
}

fn indent(s: &str, prefix: &str) -> String {
    s.lines()
        .map(|l| format!("{prefix}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Prompt for [Y/N] via stdin; returns true for 'y'/'Y'.
fn prompt_yn(prompt: &str) -> Result<bool> {
    print!("{prompt} [Y/N] > ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Apply a proposal atomically: apply patch + record ADR under one lock.
fn apply_with_adr(root: &Path, id: &str) -> Result<()> {
    // Load before acquiring lock so we know whether an ADR needs writing.
    let prop = load(root, id)?;

    let _lock = VaultLock::acquire(root)?;

    // Apply patch and mark proposal as applied.
    apply_inner(root, id)?;

    // If the proposal includes an inline ADR, append it to decision_log.yml.
    if let Some(adr) = &prop.adr_proposal {
        crate::commands::decision::add_entry_inner(
            root,
            &adr.title,
            &adr.reason,
            "accepted",
            "global",
        )?;
    }

    Ok(())
}

/// Reject a proposal atomically under one lock.
fn reject_proposal(root: &Path, id: &str) -> Result<()> {
    let _lock = VaultLock::acquire(root)?;
    let mut prop = load(root, id)?;
    if prop.status != PENDING {
        // Already applied or rejected by a concurrent triage run — skip.
        return Ok(());
    }
    prop.status = REJECTED.to_string();
    // Write back via the proposals write path (no lock re-acquire needed).
    let path = root
        .join("state/proposals")
        .join(format!("{}.yml", prop.id));
    let out = serde_yaml::to_string(&prop)?;
    crate::core::vaultio::atomic_write(&path, out.as_bytes())?;
    Ok(())
}

/// `knogg triage`: interactively approve or reject pending proposals.
pub fn triage(path: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let proposals = pending_proposals(&root)?;

    if proposals.is_empty() {
        println!("No pending proposals.");
        return Ok(());
    }

    println!("Pending proposals: {}", proposals.len());

    let mut approved = 0u32;
    let mut rejected = 0u32;
    let mut skipped = 0u32;

    for p in &proposals {
        print_proposal_summary(p);
        match prompt_yn("Approve?") {
            Ok(true) => {
                match apply_with_adr(&root, &p.id) {
                    Ok(()) => {
                        println!("  applied {}", p.id);
                        approved += 1;
                    }
                    Err(e) => eprintln!("  ERROR applying {}: {e:#}", p.id),
                }
            }
            Ok(false) => {
                match reject_proposal(&root, &p.id) {
                    Ok(()) => {
                        println!("  rejected {}", p.id);
                        rejected += 1;
                    }
                    Err(e) => eprintln!("  ERROR rejecting {}: {e:#}", p.id),
                }
            }
            Err(e) => {
                eprintln!("  stdin error: {e:#}; skipping {}", p.id);
                skipped += 1;
            }
        }
    }

    println!(
        "\nTriage done — approved: {approved}, rejected: {rejected}, skipped: {skipped}"
    );

    // Refresh the brief after bulk changes.
    if approved > 0 {
        let _ = crate::commands::brief::refresh(&root);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::proposal::{create_fat_with_policy, load};
    use crate::core::vault::init;
    use serde_json::json;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("knogg-triage-{label}-{nanos}"))
    }

    #[test]
    fn pending_proposals_excludes_terminal() {
        let root = temp_root("filter");
        init(root.to_str().unwrap(), false).unwrap();

        // Create pending proposal.
        create_fat_with_policy(
            &root,
            "state/active_context.yml",
            &json!({"focus": {"status": "done"}}),
            "r",
            None,
            None,
            false,
        )
        .unwrap();

        let pending = pending_proposals(&root).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].status, "pending");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn apply_with_adr_mutates_context_and_logs_decision() {
        let root = temp_root("adr");
        init(root.to_str().unwrap(), false).unwrap();

        let adr = crate::core::schema::AdrProposal {
            title: "Use atomic triage".to_string(),
            reason: "consistency".to_string(),
        };
        let outcome = create_fat_with_policy(
            &root,
            "state/active_context.yml",
            &json!({"focus": {"status": "done"}}),
            "triage test",
            Some(adr),
            Some("please review".to_string()),
            false,
        )
        .unwrap();

        apply_with_adr(&root, &outcome.proposal_id).unwrap();

        // Patch was applied.
        let ctx = crate::core::vault::read_active_context(&root).unwrap();
        assert_eq!(ctx.focus.status, "done");

        // Proposal marked applied.
        let prop = load(&root, &outcome.proposal_id).unwrap();
        assert_eq!(prop.status, "applied");

        // ADR was written to decision_log.
        let recent =
            crate::commands::decision::recent_summaries(&root, 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].title, "Use atomic triage");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn apply_with_no_adr_skips_decision_log() {
        let root = temp_root("no_adr");
        init(root.to_str().unwrap(), false).unwrap();

        let outcome = create_fat_with_policy(
            &root,
            "state/active_context.yml",
            &json!({"focus": {"status": "blocked"}}),
            "no adr",
            None,
            None,
            false,
        )
        .unwrap();

        apply_with_adr(&root, &outcome.proposal_id).unwrap();

        let recent =
            crate::commands::decision::recent_summaries(&root, 10).unwrap();
        assert!(recent.is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
