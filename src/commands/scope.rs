//! Capability-aware allowed scope for agents (ADR-0006).

use std::path::Path;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::commands::agents::{self, AgentRegistry};
use crate::commands::brief;
use crate::commands::plan;
use crate::commands::roles;

/// Build MCP/CLI scope payload for an agent (or project-wide if `agent` is None).
pub fn allowed_scope(root: &Path, agent: Option<&str>) -> Result<Value> {
    let brief = brief::load_or_refresh(root)?;
    let mut constraints = brief.constraints.clone();

    let Some(name) = agent else {
        return Ok(json!({
            "scope": "project",
            "constraints": constraints,
        }));
    };

    let reg: AgentRegistry = agents::load_registry(root)?;
    let entry = reg
        .agents
        .get(name)
        .ok_or_else(|| anyhow!("unknown agent '{name}'"))?;

    let role_name = agents::agent_role(root, name)?;
    let role = roles::get(root, &role_name)?;

    let mut capabilities: Vec<String> = entry
        .capabilities
        .clone();

    if capabilities.is_empty() {
        capabilities.push(format!("role:{role_name}"));
    }

    constraints.extend(role.constraints.clone());

    let open = plan::open_tasks(root)?;
    let mut denied_files: Vec<String> = Vec::new();
    for t in &open {
        if t.status != "in_progress" {
            continue;
        }
        if t.owner.as_deref() == Some(name) {
            continue;
        }
        denied_files.extend(t.files.clone());
    }

    Ok(json!({
        "scope": "agent",
        "agent": name,
        "role": role_name,
        "capabilities": capabilities,
        "responsibilities": role.responsibilities,
        "constraints": constraints,
        "denied_files": denied_files,
        "profile": serde_json::to_value(&entry.profile).unwrap_or(json!(null)),
    }))
}
