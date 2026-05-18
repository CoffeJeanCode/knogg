use std::fs;
use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use crate::core::vault::{audit_patch, resolve_path, safe_vault_path};

/// MCP protocol version this server speaks.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// `knogg mcp`: serve the MCP tools over JSON-RPC on stdio.
pub fn serve(path: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        // Notifications (no `id`) get no response.
        if let Some(response) = dispatch_line(&root, &line) {
            writeln!(stdout, "{response}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// Parse one JSON-RPC line. Returns `None` for notifications (no `id`).
fn dispatch_line(root: &Path, line: &str) -> Option<String> {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(error_response(Value::Null, -32700, &format!("parse error: {e}")))
        }
    };
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(json!({}));

    // A request without `id` is a notification: process side effects, no reply.
    let id = match req.get("id") {
        None => return None,
        Some(v) => v.clone(),
    };

    // F6: before_mcp_response hooks (e.g. ensure the brief is fresh).
    // Warnings go to stderr only — stdout carries JSON-RPC.
    if let Err(e) = crate::commands::hooks::run(root, "before_mcp_response") {
        eprintln!("hook warning: {e}");
    }

    Some(match handle(root, method, &params) {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string(),
        Err(e) => error_response(id, -32000, &e.to_string()),
    })
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

/// MCP `initialize` result.
fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "knogg",
            "version": env!("CARGO_PKG_VERSION"),
        }
    })
}

/// MCP `tools/list` result: tool descriptors with input schemas.
fn tools_list() -> Value {
    let obj = |props: Value, required: Value| {
        json!({"type": "object", "properties": props,
               "required": required, "additionalProperties": false})
    };
    json!({"tools": [
        {"name": "get_active_context",
         "description": "Return the active Vault context",
         "inputSchema": obj(json!({}), json!([]))},
        {"name": "read_vault",
         "description": "Read a safe relative path from the Vault",
         "inputSchema": obj(json!({"path": {"type": "string"}}), json!(["path"]))},
        {"name": "list_vault",
         "description": "List safe Vault paths",
         "inputSchema": json!({"type": "object",
            "properties": {"include_proposals": {"type": "boolean"}},
            "additionalProperties": false})},
        {"name": "get_tool_registry",
         "description": "Return the Vault tool registry",
         "inputSchema": obj(json!({}), json!([]))},
        {"name": "propose_state_update",
         "description": "Create a staged proposal for a state update",
         "inputSchema": obj(
            json!({"target": {"type": "string"},
                   "patch": {"type": "object"},
                   "reason": {"type": "string"}}),
            json!(["target", "patch", "reason"]))},
        {"name": "audit_commit",
         "description": "Apply a staged proposal by id",
         "inputSchema": obj(json!({"id": {"type": "string"}}), json!(["id"]))},
        {"name": "search_vault",
         "description": "Search vault files for a text query (case-insensitive)",
         "inputSchema": obj(
            json!({"query": {"type": "string"}}), json!(["query"]))},
        {"name": "list_proposals",
         "description": "List staged proposals and their status",
         "inputSchema": obj(json!({}), json!([]))},
        {"name": "propose_decision",
         "description": "Append an ADR entry to the decision log",
         "inputSchema": obj(
            json!({"title": {"type": "string"},
                   "reason": {"type": "string"},
                   "status": {"type": "string"},
                   "scope": {"type": "string"}}),
            json!(["title", "reason"]))},
        {"name": "post_message",
         "description": "Post a message to the agent message log",
         "inputSchema": obj(
            json!({"from": {"type": "string"}, "text": {"type": "string"},
                   "to": {"type": "array", "items": {"type": "string"}},
                   "reply_to": {"type": "string"}}),
            json!(["from", "text"]))},
        {"name": "get_messages",
         "description": "Read agent messages with optional filters",
         "inputSchema": json!({"type": "object",
            "properties": {
              "limit": {"type": "integer"},
              "from": {"type": "string"},
              "to": {"type": "string"},
              "status": {"type": "string"},
              "unread_for": {"type": "string"}
            },
            "additionalProperties": false})},
        {"name": "ack_message",
         "description": "Mark a message read/acked by an agent",
         "inputSchema": obj(
            json!({"id": {"type": "string"}, "by": {"type": "string"}}),
            json!(["id", "by"]))},
        {"name": "list_roles",
         "description": "List agent roles",
         "inputSchema": obj(json!({}), json!([]))},
        {"name": "get_role",
         "description": "Get an agent role spec by name",
         "inputSchema": obj(json!({"name": {"type": "string"}}), json!(["name"]))},
        {"name": "set_role",
         "description": "Create or replace an agent role spec",
         "inputSchema": obj(
            json!({"name": {"type": "string"},
                   "summary": {"type": "string"},
                   "responsibilities": {"type": "array", "items": {"type": "string"}},
                   "constraints": {"type": "array", "items": {"type": "string"}}}),
            json!(["name", "summary"]))},
        {"name": "get_agent_role",
         "description": "Get the role spec assigned to an agent",
         "inputSchema": obj(json!({"agent": {"type": "string"}}), json!(["agent"]))},
        {"name": "get_style_guides",
         "description": "Return coding conventions from core/style_guides.yml",
         "inputSchema": obj(json!({}), json!([]))},
        {"name": "get_brief",
         "description": "Return the compact project brief",
         "inputSchema": obj(json!({}), json!([]))},
        {"name": "get_next_actions",
         "description": "Return the current next actions",
         "inputSchema": obj(json!({}), json!([]))},
        {"name": "get_allowed_scope",
         "description": "Return capability-aware scope for an agent (or project if agent omitted)",
         "inputSchema": json!({"type": "object",
            "properties": {"agent": {"type": "string"}},
            "additionalProperties": false})},
        {"name": "get_current_decisions",
         "description": "Return recent decisions from the brief",
         "inputSchema": obj(json!({}), json!([]))},
        {"name": "get_agent_handoff",
         "description": "Return a rendered handoff prompt for an agent",
         "inputSchema": obj(json!({"agent": {"type": "string"}}), json!(["agent"]))},
        {"name": "set_agent_role",
         "description": "Assign a role to an agent",
         "inputSchema": obj(
            json!({"agent": {"type": "string"}, "role": {"type": "string"}}),
            json!(["agent", "role"]))},
    ]})
}

/// Route a JSON-RPC method: MCP handshake methods plus direct tool calls.
fn handle(root: &Path, method: &str, params: &Value) -> Result<Value> {
    match method {
        "initialize" => Ok(initialize_result()),
        "tools/list" => Ok(tools_list()),
        "tools/call" => {
            let name = str_param(params, "name")?;
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            // Tool failures are reported via `isError`, not a JSON-RPC error.
            let (text, is_error) = match call_tool(root, name, &args) {
                Ok(v) => (v.to_string(), false),
                Err(e) => (e.to_string(), true),
            };
            Ok(json!({
                "content": [{"type": "text", "text": text}],
                "isError": is_error,
            }))
        }
        // Backwards-compatible direct tool methods.
        other => call_tool(root, other, params),
    }
}

/// Invoke a vault tool by name.
fn call_tool(root: &Path, name: &str, params: &Value) -> Result<Value> {
    match name {
        "read_vault" => {
            let target = str_param(params, "path")?;
            read_vault(root, target)
        }
        "get_active_context" => read_vault(root, "state/active_context.yml"),
        "list_vault" => {
            let include_proposals = params
                .get("include_proposals")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            list_vault(root, include_proposals)
        }
        // Returns only the registry mappings — never the template contents.
        "get_tool_registry" => read_vault(root, "plans/tool_registry.yml"),
        "propose_state_update" => {
            let target = str_param(params, "target")?;
            let patch = params
                .get("patch")
                .cloned()
                .ok_or_else(|| anyhow!("missing param 'patch'"))?;
            let reason = str_param(params, "reason").unwrap_or("");

            // Soft audit: report issues but still stage the proposal.
            let audit = audit_patch(&patch);
            // Staged only — never mutates active_context.yml directly.
            let id = crate::commands::proposal::create(root, target, &patch, reason)?;
            Ok(json!({
                "proposal_id": id,
                "status": "pending",
                "audit_ok": audit.is_ok(),
                "audit_message": audit.err().map(|e| e.to_string()),
            }))
        }
        "audit_commit" => {
            // Apply a staged proposal by id (re-audits before applying).
            let id = str_param(params, "id")?;
            crate::commands::proposal::apply(root, id)?;
            // Keep the brief fresh silently (no stdout — JSON-RPC channel).
            let _ = crate::commands::brief::refresh(root);
            Ok(json!({"committed": true, "proposal_id": id}))
        }
        "get_style_guides" => crate::commands::style::guides_json(root),
        "get_brief" => {
            let brief = crate::commands::brief::load_or_refresh(root)?;
            serde_json::to_value(&brief).map_err(|e| anyhow!("serializing brief: {e}"))
        }
        "get_next_actions" => {
            let brief = crate::commands::brief::load_or_refresh(root)?;
            Ok(json!({ "next_actions": brief.next_actions }))
        }
        "get_allowed_scope" => {
            let agent = optional_str(params, "agent");
            crate::commands::scope::allowed_scope(root, agent.as_deref())
        }
        "get_current_decisions" => {
            let brief = crate::commands::brief::load_or_refresh(root)?;
            Ok(json!({ "decisions": brief.recent_decisions }))
        }
        "get_agent_handoff" => {
            let agent = str_param(params, "agent")?;
            let prompt = crate::commands::handoff::render(root, agent)?;
            Ok(json!({"agent": agent, "prompt": prompt}))
        }
        "search_vault" => {
            let query = str_param(params, "query")?;
            search_vault(root, query)
        }
        "list_proposals" => {
            let items: Vec<Value> = crate::commands::proposal::all(root)?
                .into_iter()
                .map(|p| {
                    json!({"id": p.id, "status": p.status,
                           "target": p.target, "reason": p.reason,
                           "project": p.project})
                })
                .collect();
            Ok(json!({ "proposals": items }))
        }
        "propose_decision" => {
            let title = str_param(params, "title")?;
            let reason = str_param(params, "reason")?;
            let status = params
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("proposed");
            let scope = params
                .get("scope")
                .and_then(Value::as_str)
                .unwrap_or("global");
            let id = crate::commands::decision::add_entry(root, title, reason, status, scope)?;
            Ok(json!({"decision_id": id, "status": status}))
        }
        "post_message" => {
            let from = str_param(params, "from")?;
            let text = str_param(params, "text")?;
            let to = params
                .get("to")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .filter(|v| !v.is_empty());
            let reply_to = params
                .get("reply_to")
                .and_then(Value::as_str)
                .map(String::from);
            let id = crate::commands::messages::post(root, from, text, to, reply_to)?;
            Ok(json!({"message_id": id}))
        }
        "get_messages" => {
            let filter = crate::commands::messages::MessageFilter {
                from: optional_str(params, "from"),
                to: optional_str(params, "to"),
                status: optional_str(params, "status"),
                unread_for: optional_str(params, "unread_for"),
                limit: params
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map(|n| n as usize),
            };
            crate::commands::messages::filtered_json(root, &filter)
        }
        "ack_message" => {
            let id = str_param(params, "id")?;
            let by = str_param(params, "by")?;
            crate::commands::messages::ack(root, id, by)?;
            Ok(json!({"message_id": id, "acked_by": by}))
        }
        "list_roles" => crate::commands::roles::all_json(root),
        "get_role" => {
            let name = str_param(params, "name")?;
            crate::commands::roles::role_json(root, name)
        }
        "set_role" => {
            let name = str_param(params, "name")?;
            let summary = str_param(params, "summary")?;
            let resp = str_vec(params, "responsibilities");
            let constr = str_vec(params, "constraints");
            crate::commands::roles::set_entry(root, name, summary, resp, constr)?;
            Ok(json!({"role": name, "set": true}))
        }
        "get_agent_role" => {
            let agent = str_param(params, "agent")?;
            let role_name = crate::commands::agents::agent_role(root, agent)?;
            crate::commands::roles::role_json(root, &role_name)
        }
        "set_agent_role" => {
            let agent = str_param(params, "agent")?;
            let role = str_param(params, "role")?;
            crate::commands::agents::set_agent_role(root, agent, role)?;
            Ok(json!({"agent": agent, "role": role}))
        }
        other => bail!("unknown method '{other}'"),
    }
}

fn str_param<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing or invalid param '{key}'"))
}

fn optional_str(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(String::from)
}

/// Extract an optional array-of-strings param (missing -> empty).
fn str_vec(params: &Value, key: &str) -> Vec<String> {
    params
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

/// `read_vault`: read a YAML file inside the vault and return it as JSON.
pub fn read_vault(root: &Path, target: &str) -> Result<Value> {
    let path = safe_vault_path(root, target)?;
    let raw = fs::read_to_string(&path)
        .map_err(|e| anyhow!("reading {}: {e}", path.display()))?;
    let yaml: serde_yaml::Value = serde_yaml::from_str(&raw)
        .map_err(|e| anyhow!("parsing {}: {e}", path.display()))?;
    serde_json::to_value(yaml).map_err(|e| anyhow!("converting {} to JSON: {e}", path.display()))
}

/// `list_vault`: list safe, vault-relative file paths.
///
/// `backups/` is never listed; `state/proposals/` is listed only when
/// `include_proposals` is true.
pub fn list_vault(root: &Path, include_proposals: bool) -> Result<Value> {
    let mut paths = Vec::new();
    collect_paths(root, root, include_proposals, &mut paths)?;
    paths.sort();
    Ok(json!({ "paths": paths }))
}

fn collect_paths(
    root: &Path,
    dir: &Path,
    include_proposals: bool,
    out: &mut Vec<String>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if path.is_dir() {
            if name == "backups" || (name == "proposals" && !include_proposals) {
                continue;
            }
            collect_paths(root, &path, include_proposals, out)?;
        } else {
            // Skip the lock file and any in-flight atomic-write temp files.
            if name == ".lock" || name.ends_with(".tmp") {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(())
}

/// `search_vault`: case-insensitive text search across vault files.
pub fn search_vault(root: &Path, query: &str) -> Result<Value> {
    let needle = query.to_lowercase();
    let mut paths = Vec::new();
    collect_paths(root, root, true, &mut paths)?;
    paths.sort();

    let mut matches = Vec::new();
    for rel in paths {
        // Non-UTF-8 / binary files are skipped silently.
        let Ok(content) = fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        for (i, line) in content.lines().enumerate() {
            if line.to_lowercase().contains(&needle) {
                matches.push(json!({
                    "path": rel,
                    "line": i + 1,
                    "text": line.trim(),
                }));
            }
        }
    }
    Ok(json!({ "query": query, "matches": matches }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use crate::core::vault::init;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("vault-mcp-{label}-{nanos}"))
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        let root = PathBuf::from("/tmp/vault");
        assert!(safe_vault_path(&root, "../etc/passwd").is_err());
        assert!(safe_vault_path(&root, "/etc/passwd").is_err());
        assert!(safe_vault_path(&root, "state/../../x").is_err());
        assert!(safe_vault_path(&root, "state/active_context.yml").is_ok());
    }

    #[test]
    fn audit_blocks_invalid_status() {
        use crate::core::vault::ALLOWED_STATUS;
        let bad = json!({"focus": {"status": "WRONG"}});
        assert!(audit_patch(&bad).is_err());
        for s in ALLOWED_STATUS {
            let ok = json!({"focus": {"status": s}});
            assert!(audit_patch(&ok).is_ok());
        }
    }

    #[test]
    fn read_vault_returns_json() {
        let root = temp_root("read");
        init(root.to_str().unwrap(), false).unwrap();
        let v = read_vault(&root, "state/active_context.yml").unwrap();
        assert_eq!(v["project"]["name"], "knogg");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn valid_patch_applies_invalid_blocks() {
        use crate::core::vault::apply_patch;
        let root = temp_root("apply");
        init(root.to_str().unwrap(), false).unwrap();
        let target = "state/active_context.yml";

        // Invalid status is blocked before any write.
        let bad = json!({"focus": {"status": "nope"}});
        assert!(audit_patch(&bad).is_err());

        // Valid patch is audited then applied.
        let good = json!({"focus": {"status": "done"}});
        audit_patch(&good).unwrap();
        apply_patch(&root, target, &good).unwrap();

        let after = read_vault(&root, target).unwrap();
        assert_eq!(after["focus"]["status"], "done");
        // Untouched fields survive the merge.
        assert_eq!(after["project"]["name"], "knogg");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn get_active_context_returns_resolved_context() {
        let root = temp_root("getctx");
        init(root.to_str().unwrap(), false).unwrap();
        let v = handle(&root, "get_active_context", &json!({})).unwrap();
        assert_eq!(v["project"]["name"], "knogg");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn list_vault_hides_backups_and_proposals_by_default() {
        let root = temp_root("listvault");
        init(root.to_str().unwrap(), false).unwrap();
        crate::commands::proposal::create(
            &root,
            "state/active_context.yml",
            &json!({"focus": {"status": "done"}}),
            "r",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("backups/stamp")).unwrap();
        std::fs::write(root.join("backups/stamp/old.yml"), "x").unwrap();

        let default = handle(&root, "list_vault", &json!({})).unwrap();
        let listed = default["paths"].as_array().unwrap();
        assert!(listed.iter().all(|p| {
            let s = p.as_str().unwrap();
            !s.contains("backups/") && !s.contains("proposals/")
        }));
        assert!(listed.iter().any(|p| p == "state/active_context.yml"));

        let with = handle(&root, "list_vault", &json!({"include_proposals": true})).unwrap();
        assert!(with["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p.as_str().unwrap().contains("proposals/")));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn get_tool_registry_returns_mappings() {
        let root = temp_root("registry");
        init(root.to_str().unwrap(), false).unwrap();
        let v = handle(&root, "get_tool_registry", &json!({})).unwrap();
        assert!(v["tools"].is_array());
        assert!(v["tools"][0]["output"].is_string());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn propose_stages_proposal_without_direct_write() {
        let root = temp_root("flow");
        init(root.to_str().unwrap(), false).unwrap();

        let before = read_vault(&root, "state/active_context.yml").unwrap();
        let propose = handle(
            &root,
            "propose_state_update",
            &json!({"target": "state/active_context.yml",
                    "patch": {"focus": {"status": "blocked"}},
                    "reason": "waiting on review"}),
        )
        .unwrap();
        assert_eq!(propose["status"], "pending");
        let id = propose["proposal_id"].as_str().unwrap().to_string();

        // active_context.yml is untouched until the proposal is committed.
        let mid = read_vault(&root, "state/active_context.yml").unwrap();
        assert_eq!(before["focus"]["status"], mid["focus"]["status"]);

        handle(&root, "audit_commit", &json!({ "id": id })).unwrap();
        let after = read_vault(&root, "state/active_context.yml").unwrap();
        assert_eq!(after["focus"]["status"], "blocked");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn initialize_returns_protocol_info() {
        let root = PathBuf::from("/tmp/unused");
        let v = handle(&root, "initialize", &json!({})).unwrap();
        assert_eq!(v["protocolVersion"], "2024-11-05");
        assert_eq!(v["serverInfo"]["name"], "knogg");
        assert!(v["capabilities"]["tools"].is_object());
    }

    #[test]
    fn notification_gets_no_response() {
        let root = PathBuf::from("/tmp/unused");
        let note = r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#;
        assert!(dispatch_line(&root, note).is_none());
        // A request with an `id` still gets a response.
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        assert!(dispatch_line(&root, req).is_some());
    }

    #[test]
    fn tools_list_advertises_tools() {
        let root = PathBuf::from("/tmp/unused");
        let v = handle(&root, "tools/list", &json!({})).unwrap();
        let names: Vec<&str> = v["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"get_active_context"));
        assert!(names.contains(&"read_vault"));
    }

    #[test]
    fn tools_call_invokes_tool() {
        let root = temp_root("toolscall");
        init(root.to_str().unwrap(), false).unwrap();
        let v = handle(
            &root,
            "tools/call",
            &json!({"name": "get_active_context", "arguments": {}}),
        )
        .unwrap();
        assert_eq!(v["isError"], false);
        let text = v["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["project"]["name"], "knogg");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn tools_call_unknown_tool_is_error() {
        let root = PathBuf::from("/tmp/unused");
        let v = handle(
            &root,
            "tools/call",
            &json!({"name": "does_not_exist", "arguments": {}}),
        )
        .unwrap();
        assert_eq!(v["isError"], true);
    }

    #[test]
    fn direct_methods_still_work() {
        let root = temp_root("direct");
        init(root.to_str().unwrap(), false).unwrap();
        let ctx = handle(&root, "get_active_context", &json!({})).unwrap();
        assert_eq!(ctx["project"]["name"], "knogg");
        let rv = handle(
            &root,
            "read_vault",
            &json!({"path": "state/active_context.yml"}),
        )
        .unwrap();
        assert_eq!(rv["project"]["name"], "knogg");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn search_vault_finds_text() {
        let root = temp_root("search");
        init(root.to_str().unwrap(), false).unwrap();
        // case-insensitive: `Focus` matches the `focus:` key in YAML.
        let v = handle(&root, "search_vault", &json!({"query": "FOCUS"})).unwrap();
        let matches = v["matches"].as_array().unwrap();
        assert!(!matches.is_empty());
        assert!(matches
            .iter()
            .any(|m| m["path"] == "state/active_context.yml"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn propose_decision_appends_adr() {
        let root = temp_root("decision");
        init(root.to_str().unwrap(), false).unwrap();
        let v = handle(
            &root,
            "propose_decision",
            &json!({"title": "Use X", "reason": "because"}),
        )
        .unwrap();
        assert_eq!(v["decision_id"], "ADR-0001");
        assert_eq!(v["status"], "proposed");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn messages_post_and_read() {
        let root = temp_root("msg");
        init(root.to_str().unwrap(), false).unwrap();
        handle(&root, "post_message", &json!({"from": "cursor", "text": "hi"})).unwrap();
        let v = handle(&root, "get_messages", &json!({})).unwrap();
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["from"], "cursor");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ack_message_marks_read() {
        let root = temp_root("ack");
        init(root.to_str().unwrap(), false).unwrap();
        let posted = handle(
            &root,
            "post_message",
            &json!({"from": "claude", "text": "go", "to": ["cursor"]}),
        )
        .unwrap();
        let id = posted["message_id"].as_str().unwrap();
        handle(
            &root,
            "ack_message",
            &json!({"id": id, "by": "cursor"}),
        )
        .unwrap();
        let unread = handle(
            &root,
            "get_messages",
            &json!({"unread_for": "cursor"}),
        )
        .unwrap();
        assert!(unread["messages"].as_array().unwrap().is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn list_proposals_reflects_staged() {
        let root = temp_root("listprop");
        init(root.to_str().unwrap(), false).unwrap();
        handle(
            &root,
            "propose_state_update",
            &json!({"target": "state/active_context.yml",
                    "patch": {"focus": {"status": "done"}},
                    "reason": "r"}),
        )
        .unwrap();
        let v = handle(&root, "list_proposals", &json!({})).unwrap();
        let items = v["proposals"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["status"], "pending");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn roles_set_get_list() {
        let root = temp_root("roles");
        init(root.to_str().unwrap(), false).unwrap();

        // Seeded default roles are visible.
        let listed = handle(&root, "list_roles", &json!({})).unwrap();
        assert!(!listed["roles"].as_array().unwrap().is_empty());

        // set_role then get_role by name.
        handle(
            &root,
            "set_role",
            &json!({"name": "tester", "summary": "Runs tests",
                    "responsibilities": ["run cargo test"]}),
        )
        .unwrap();
        let role = handle(&root, "get_role", &json!({"name": "tester"})).unwrap();
        assert_eq!(role["summary"], "Runs tests");
        assert_eq!(role["responsibilities"][0], "run cargo test");

        // Unknown role -> tools/call reports isError.
        let miss = handle(
            &root,
            "tools/call",
            &json!({"name": "get_role", "arguments": {"name": "ghost"}}),
        )
        .unwrap();
        assert_eq!(miss["isError"], true);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn agent_role_link_via_mcp() {
        let root = temp_root("agentrole");
        init(root.to_str().unwrap(), false).unwrap();

        // Seeded: claude -> reviewer.
        let v = handle(&root, "get_agent_role", &json!({"agent": "claude"})).unwrap();
        assert_eq!(v["name"], "reviewer");
        assert!(!v["responsibilities"].as_array().unwrap().is_empty());

        // Reassign and read back.
        handle(
            &root,
            "set_agent_role",
            &json!({"agent": "cursor", "role": "reviewer"}),
        )
        .unwrap();
        let v = handle(&root, "get_agent_role", &json!({"agent": "cursor"})).unwrap();
        assert_eq!(v["name"], "reviewer");
        fs::remove_dir_all(&root).ok();
    }
}
