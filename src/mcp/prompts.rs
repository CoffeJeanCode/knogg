use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

/// System manual injected at the top of every knogg-task prompt.
const KNOGG_MANUAL: &str = "\
SISTEMA KNOGG: Estás operando dentro de un Context Broker local. \
TUS REGLAS: \
1. NO edites archivos dentro de .knogg/ directamente. \
2. Para marcar una tarea como completada, proponer un ADR o dejar un mensaje al humano, \
DEBES usar obligatoriamente la herramienta 'propose_state_update'. \
No uses otras herramientas.";

/// MCP `prompts/list` result.
pub fn list_prompts() -> Value {
    json!({"prompts": [
        {
            "name": "knogg-task",
            "description": "Injects the Knogg system manual and the current task context into a prompt",
            "arguments": [
                {
                    "name": "agent",
                    "description": "Name of the requesting agent (e.g. cursor, claude)",
                    "required": false
                }
            ]
        }
    ]})
}

/// MCP `prompts/get` — dispatch by prompt name.
pub fn get_prompt(root: &Path, name: &str, arguments: &Value) -> Result<Value> {
    match name {
        "knogg-task" => build_task_prompt(root, arguments),
        other => bail!("unknown prompt '{other}' (available: knogg-task)"),
    }
}

fn build_task_prompt(root: &Path, arguments: &Value) -> Result<Value> {
    let requesting_agent = arguments
        .get("agent")
        .and_then(Value::as_str)
        .unwrap_or("");

    let raw = fs::read_to_string(root.join("state/active_context.yml"))
        .map_err(|e| anyhow!("reading active_context.yml: {e}"))?;

    // Use serde_yaml::Value for forward-compat: unknown fields are preserved.
    let ctx: serde_yaml::Value =
        serde_yaml::from_str(&raw).map_err(|e| anyhow!("parsing active_context.yml: {e}"))?;

    let focus = ctx.get("focus");
    let task = str_field(focus, "task").unwrap_or("(no task set)");
    let stage = str_field(focus, "stage").unwrap_or("");
    let status = str_field(focus, "status").unwrap_or("todo");
    let owner = str_field(focus, "owner").unwrap_or("");

    let next_actions: Vec<&str> = ctx
        .get("next_actions")
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut text = format!("{KNOGG_MANUAL}\n\n");

    // Warn explicitly when the task is owned by a different agent.
    if !owner.is_empty() && !requesting_agent.is_empty() && owner != requesting_agent {
        text.push_str(&format!(
            "ADVERTENCIA: La tarea actual tiene owner '{owner}'. \
             Estás operando como '{requesting_agent}'. \
             Coordina con el owner antes de modificar el estado.\n\n"
        ));
    }

    text.push_str("## Tarea Actual\n");
    if !stage.is_empty() {
        text.push_str(&format!("- Stage: {stage}\n"));
    }
    text.push_str(&format!("- Task: {task}\n"));
    text.push_str(&format!("- Status: {status}\n"));
    if !owner.is_empty() {
        text.push_str(&format!("- Owner: {owner}\n"));
    }

    if !next_actions.is_empty() {
        text.push_str("\n## Next Actions\n");
        for a in &next_actions {
            text.push_str(&format!("- {a}\n"));
        }
    }

    Ok(json!({
        "description": "Knogg task context with system manual",
        "messages": [{
            "role": "user",
            "content": {
                "type": "text",
                "text": text,
            }
        }]
    }))
}

fn str_field<'a>(parent: Option<&'a serde_yaml::Value>, key: &str) -> Option<&'a str> {
    parent?.get(key)?.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::vault::init;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("knogg-prompt-{label}-{nanos}"))
    }

    #[test]
    fn list_exposes_knogg_task() {
        let v = list_prompts();
        let prompts = v["prompts"].as_array().unwrap();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0]["name"], "knogg-task");
    }

    #[test]
    fn knogg_task_contains_manual_and_task() {
        let root = temp_root("task");
        init(root.to_str().unwrap(), false).unwrap();
        let v = get_prompt(&root, "knogg-task", &json!({"agent": "cursor"})).unwrap();
        let text = v["messages"][0]["content"]["text"].as_str().unwrap();
        assert!(text.contains("SISTEMA KNOGG"));
        assert!(text.contains("propose_state_update"));
        assert!(text.contains("## Tarea Actual"));
        assert!(text.contains("Stage 1"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn owner_mismatch_inserts_warning() {
        let root = temp_root("owner");
        init(root.to_str().unwrap(), false).unwrap();

        // Write an active_context with a different owner.
        let ctx = "project:\n  name: knogg\nfocus:\n  stage: S1\n  task: Do X\n  status: in_progress\n  owner: cursor\nnext_actions: []\nhandoff:\n  summary: \"\"\n";
        fs::write(root.join("state/active_context.yml"), ctx).unwrap();

        let v = get_prompt(&root, "knogg-task", &json!({"agent": "claude"})).unwrap();
        let text = v["messages"][0]["content"]["text"].as_str().unwrap();
        assert!(text.contains("ADVERTENCIA"));
        assert!(text.contains("cursor"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn no_warning_when_agent_is_owner() {
        let root = temp_root("same_owner");
        init(root.to_str().unwrap(), false).unwrap();

        let ctx = "project:\n  name: knogg\nfocus:\n  stage: S1\n  task: Do X\n  status: in_progress\n  owner: cursor\nnext_actions: []\nhandoff:\n  summary: \"\"\n";
        fs::write(root.join("state/active_context.yml"), ctx).unwrap();

        let v = get_prompt(&root, "knogg-task", &json!({"agent": "cursor"})).unwrap();
        let text = v["messages"][0]["content"]["text"].as_str().unwrap();
        assert!(!text.contains("ADVERTENCIA"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unknown_prompt_is_error() {
        let root = PathBuf::from("/tmp/unused");
        assert!(get_prompt(&root, "does_not_exist", &json!({})).is_err());
    }
}
