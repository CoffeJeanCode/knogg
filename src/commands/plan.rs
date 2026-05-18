//! Read and update partitioned tasks in `plans/master_plan.yml`.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::core::vaultio::{atomic_write, VaultLock};

/// One partitioned task (stages_append or add_task).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlanTask {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default = "default_task_status")]
    pub status: String,
    #[serde(default, rename = "allowed_roles")]
    pub allowed_roles: Vec<String>,
}

fn default_task_status() -> String {
    "todo".to_string()
}

#[derive(Debug, Deserialize, Serialize)]
struct MasterPlanDoc {
    #[serde(default)]
    stages: Vec<Value>,
    #[serde(default)]
    stages_append: Vec<Value>,
    #[serde(default)]
    add_task: Option<Value>,
}

fn plan_path(root: &Path) -> std::path::PathBuf {
    root.join("plans/master_plan.yml")
}

fn load_doc(root: &Path) -> Result<MasterPlanDoc> {
    let raw = fs::read_to_string(plan_path(root))
        .map_err(|e| anyhow!("reading master_plan.yml: {e}"))?;
    serde_yaml::from_str(&raw).map_err(|e| anyhow!("parsing master_plan.yml: {e}"))
}

fn write_doc(root: &Path, doc: &MasterPlanDoc) -> Result<()> {
    let out = serde_yaml::to_string(doc).map_err(|e| anyhow!("serializing master_plan: {e}"))?;
    atomic_write(&plan_path(root), out.as_bytes())
}

fn task_from_value(v: &Value) -> Option<PlanTask> {
    if !v.get("id").and_then(Value::as_str).is_some() {
        return None;
    }
    serde_yaml::from_value(v.clone()).ok()
}

fn tasks_in_stage_mut(stage: &mut Value) -> Option<&mut Vec<Value>> {
    let Value::Mapping(map) = stage else {
        return None;
    };
    let tasks = map.get_mut(Value::from("tasks"))?;
    let Value::Sequence(seq) = tasks else {
        return None;
    };
    Some(seq)
}

fn update_task(
    doc: &mut MasterPlanDoc,
    task_id: &str,
    mutator: impl FnOnce(&mut PlanTask) -> Result<()>,
) -> Result<bool> {
    for stage in &mut doc.stages_append {
        if let Some(tasks) = tasks_in_stage_mut(stage) {
            for t in tasks {
                if let Some(mut task) = task_from_value(t) {
                    if task.id == task_id {
                        mutator(&mut task)?;
                        *t = serde_yaml::to_value(&task)?;
                        return Ok(true);
                    }
                }
            }
        }
    }
    if let Some(ref mut at) = doc.add_task {
        if let Some(task_v) = at.get_mut(Value::from("task")) {
            if let Some(mut task) = task_from_value(task_v) {
                if task.id == task_id {
                    mutator(&mut task)?;
                    *task_v = serde_yaml::to_value(&task)?;
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

/// All structured tasks from stages_append and add_task.
pub fn all_tasks(root: &Path) -> Result<Vec<PlanTask>> {
    let doc = load_doc(root)?;
    let mut out = Vec::new();
    for stage in &doc.stages_append {
        if let Some(tasks) = stage.get("tasks").and_then(Value::as_sequence) {
            for t in tasks {
                if let Some(task) = task_from_value(t) {
                    out.push(task);
                }
            }
        }
    }
    if let Some(at) = &doc.add_task {
        if let Some(task) = at.get("task").and_then(task_from_value) {
            out.push(task);
        }
    }
    Ok(out)
}

/// Open tasks: `todo` or `in_progress`.
pub fn open_tasks(root: &Path) -> Result<Vec<PlanTask>> {
    Ok(all_tasks(root)?
        .into_iter()
        .filter(|t| t.status == "todo" || t.status == "in_progress")
        .collect())
}

/// Whether two file globs might target the same path (simple `*` prefix rule).
pub fn globs_overlap(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    for (pat, other) in [(a, b), (b, a)] {
        if let Some(i) = pat.find('*') {
            if other.starts_with(&pat[..i]) {
                return true;
            }
        }
    }
    false
}

/// Pairs of open tasks with different owners sharing a file glob.
pub fn overlap_conflicts(root: &Path) -> Result<Vec<(String, String, String)>> {
    let open = open_tasks(root)?;
    let mut conflicts = Vec::new();
    for i in 0..open.len() {
        for j in (i + 1)..open.len() {
            let a = &open[i];
            let b = &open[j];
            let oa = a.owner.as_deref().unwrap_or("");
            let ob = b.owner.as_deref().unwrap_or("");
            if oa.is_empty() || ob.is_empty() || oa == ob {
                continue;
            }
            for fa in &a.files {
                for fb in &b.files {
                    if globs_overlap(fa, fb) {
                        conflicts.push((a.id.clone(), b.id.clone(), format!("{fa} ∩ {fb}")));
                    }
                }
            }
        }
    }
    Ok(conflicts)
}

/// Claim a task: set status `in_progress`.
pub fn claim(root: &Path, task_id: &str, agent: &str) -> Result<()> {
    let _lock = VaultLock::acquire(root)?;
    let mut doc = load_doc(root)?;
    let found = update_task(&mut doc, task_id, |t| {
        if t.status != "todo" && t.status != "in_progress" {
            bail!("task {task_id} is '{}' — cannot claim", t.status);
        }
        if let Some(ref owner) = t.owner {
            if owner != agent {
                bail!("task {task_id} is owned by '{owner}', not '{agent}'");
            }
        }
        t.status = "in_progress".to_string();
        if t.owner.is_none() {
            t.owner = Some(agent.to_string());
        }
        Ok(())
    })?;
    if !found {
        bail!("task '{task_id}' not found in master_plan.yml");
    }
    write_doc(root, &doc)
}

/// Release a task: set status `done`.
pub fn release(root: &Path, task_id: &str, agent: &str) -> Result<()> {
    let _lock = VaultLock::acquire(root)?;
    let mut doc = load_doc(root)?;
    let found = update_task(&mut doc, task_id, |t| {
        if t.status != "in_progress" {
            bail!("task {task_id} is '{}' — only in_progress can be released", t.status);
        }
        if let Some(ref owner) = t.owner {
            if owner != agent {
                bail!("task {task_id} is owned by '{owner}', not '{agent}'");
            }
        }
        t.status = "done".to_string();
        Ok(())
    })?;
    if !found {
        bail!("task '{task_id}' not found in master_plan.yml");
    }
    write_doc(root, &doc)
}

// ---- CLI -------------------------------------------------------------------

pub fn cmd_list(path: &str) -> Result<()> {
    let root = crate::core::vault::resolve_path(path)?;
    for t in all_tasks(&root)? {
        let owner = t.owner.as_deref().unwrap_or("-");
        println!(
            "{}  {:12}  {:12}  {}",
            t.id,
            owner,
            t.status,
            t.desc.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

pub fn cmd_claim(path: &str, task_id: &str, agent: &str) -> Result<()> {
    let root = crate::core::vault::resolve_path(path)?;
    claim(&root, task_id, agent)?;
    println!("claimed {task_id} by {agent}");
    Ok(())
}

pub fn cmd_release(path: &str, task_id: &str, agent: &str) -> Result<()> {
    let root = crate::core::vault::resolve_path(path)?;
    release(&root, task_id, agent)?;
    println!("released {task_id} (done)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::vault::init;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("knogg-plan-{label}-{nanos}"))
    }

    fn write_partitioned_plan(root: &Path) {
        let yaml = r#"stages: []
stages_append:
  - name: Stage X
    tasks:
      - id: t-a
        owner: cursor
        status: todo
        files: [src/*.rs]
      - id: t-b
        owner: opencode
        status: in_progress
        files: [src/cli.rs]
"#;
        fs::write(root.join("plans/master_plan.yml"), yaml).unwrap();
    }

    #[test]
    fn globs_overlap_detects_shared_prefix() {
        assert!(globs_overlap("src/*.rs", "src/cli.rs"));
        assert!(!globs_overlap("src/commands/a.rs", ".knogg/**"));
    }

    #[test]
    fn overlap_conflicts_reports_different_owners() {
        let root = temp_root("overlap");
        init(root.to_str().unwrap(), false).unwrap();
        write_partitioned_plan(&root);
        let c = overlap_conflicts(&root).unwrap();
        assert!(!c.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn claim_and_release_roundtrip() {
        let root = temp_root("claim");
        init(root.to_str().unwrap(), false).unwrap();
        write_partitioned_plan(&root);
        claim(&root, "t-a", "cursor").unwrap();
        assert_eq!(
            all_tasks(&root)
                .unwrap()
                .iter()
                .find(|t| t.id == "t-a")
                .unwrap()
                .status,
            "in_progress"
        );
        release(&root, "t-a", "cursor").unwrap();
        assert_eq!(
            all_tasks(&root)
                .unwrap()
                .iter()
                .find(|t| t.id == "t-a")
                .unwrap()
                .status,
            "done"
        );
        fs::remove_dir_all(&root).ok();
    }
}
