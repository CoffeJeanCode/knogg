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
         "description": "Consolidated context: focus, next_actions, decisions, scope, handoff, inbox (ADR-0010)",
         "inputSchema": json!({"type": "object",
            "properties": {"agent": {"type": "string"}},
            "additionalProperties": false})},
        {"name": "read_vault",
         "description": "Read a vault file; optional 1-based line range",
         "inputSchema": json!({"type": "object",
            "properties": {
              "path": {"type": "string"},
              "start_line": {"type": "integer"},
              "end_line": {"type": "integer"}
            },
            "required": ["path"],
            "additionalProperties": false})},
        {"name": "list_vault",
         "description": "List safe Vault paths",
         "inputSchema": json!({"type": "object",
            "properties": {"include_proposals": {"type": "boolean"}},
            "additionalProperties": false})},
        {"name": "search_vault",
         "description": "Search vault files for a text query (case-insensitive)",
         "inputSchema": obj(
            json!({"query": {"type": "string"}}), json!(["query"]))},
        {"name": "get_tool_registry",
         "description": "Return the Vault tool registry",
         "inputSchema": obj(json!({}), json!([]))},
        {"name": "propose_state_update",
         "description": "Stage or auto-apply a state update (risk-tiered, ADR-0011)",
         "inputSchema": obj(
            json!({"target": {"type": "string"},
                   "patch": {"type": "object"},
                   "reason": {"type": "string"}}),
            json!(["target", "patch", "reason"]))},
        {"name": "audit_commit",
         "description": "Apply a staged proposal by id",
         "inputSchema": obj(json!({"id": {"type": "string"}}), json!(["id"]))},
        {"name": "messages",
         "description": "List open inbox or post a message (action=list|post)",
         "inputSchema": json!({"type": "object",
            "properties": {
              "action": {"type": "string"},
              "agent": {"type": "string"},
              "from": {"type": "string"},
              "text": {"type": "string"},
              "to": {"type": "array", "items": {"type": "string"}},
              "reply_to": {"type": "string"},
              "status": {"type": "string"},
              "limit": {"type": "integer"}
            },
            "required": ["action"],
            "additionalProperties": false})},
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

/// Invoke a vault tool by name (v1 surface: 8 tools, ADR-0010).
fn call_tool(root: &Path, name: &str, params: &Value) -> Result<Value> {
    match name {
        "get_active_context" => get_active_context(root, params),
        "read_vault" => {
            let target = str_param(params, "path")?;
            let start = params.get("start_line").and_then(Value::as_u64);
            let end = params.get("end_line").and_then(Value::as_u64);
            read_vault(root, target, start, end)
        }
        "list_vault" => {
            let include_proposals = params
                .get("include_proposals")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            list_vault(root, include_proposals)
        }
        "search_vault" => {
            let query = str_param(params, "query")?;
            search_vault(root, query)
        }
        "get_tool_registry" => read_vault(root, "plans/tool_registry.yml", None, None),
        "propose_state_update" => {
            let target = str_param(params, "target")?;
            let patch = params
                .get("patch")
                .cloned()
                .ok_or_else(|| anyhow!("missing param 'patch'"))?;
            let reason = params
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("");

            let audit = audit_patch(&patch);
            let cfg = crate::core::config::load().unwrap_or_default();
            let outcome = crate::commands::proposal::create_with_policy(
                root,
                target,
                &patch,
                reason,
                cfg.proposals.autoapply_low,
            )?;
            if outcome.status == "applied" {
                let _ = crate::commands::brief::refresh(root);
            }
            Ok(json!({
                "proposal_id": outcome.proposal_id,
                "status": outcome.status,
                "superseded": outcome.superseded,
                "audit_ok": audit.is_ok(),
                "audit_message": audit.err().map(|e| e.to_string()),
            }))
        }
        "audit_commit" => {
            let id = str_param(params, "id")?;
            crate::commands::proposal::apply(root, id)?;
            let _ = crate::commands::brief::refresh(root);
            Ok(json!({"committed": true, "proposal_id": id}))
        }
        "messages" => messages_tool(root, params),
        // Legacy tool names → helpful errors after v1 prune.
        legacy if is_legacy_tool(legacy) => {
            bail!("tool '{legacy}' removed in v1 MCP surface — use get_active_context, messages, or CLI")
        }
        other => bail!("unknown method '{other}'"),
    }
}

fn is_legacy_tool(name: &str) -> bool {
    matches!(
        name,
        "get_brief"
            | "get_next_actions"
            | "get_current_decisions"
            | "get_allowed_scope"
            | "get_agent_handoff"
            | "get_style_guides"
            | "post_message"
            | "get_messages"
            | "ack_message"
            | "list_proposals"
            | "propose_decision"
            | "list_roles"
            | "get_role"
            | "set_role"
            | "get_agent_role"
            | "set_agent_role"
    )
}

/// Terse consolidated agent context (ADR-0010 / Stage 10B).
fn get_active_context(root: &Path, params: &Value) -> Result<Value> {
    let agent = optional_str(params, "agent");
    let brief = crate::commands::brief::load_or_refresh(root)?;
    let scope = crate::commands::scope::allowed_scope(root, agent.as_deref())?;
    let handoff_agent = agent.as_deref().unwrap_or("cursor");
    let handoff = crate::commands::handoff::render(root, handoff_agent).ok();
    let inbox = crate::commands::messages::inbox_terse(root, agent.as_deref())?;
    Ok(json!({
        "pj": brief.project,
        "fo": brief.focus,
        "nx": brief.next_actions,
        "ct": brief.constraints,
        "dc": brief.recent_decisions,
        "sc": scope,
        "ho": handoff,
        "ib": inbox,
        "hs": brief.handoff_summary,
    }))
}

fn messages_tool(root: &Path, params: &Value) -> Result<Value> {
    let action = str_param(params, "action")?;
    match action {
        "list" => {
            let filter = crate::commands::messages::MessageFilter {
                from: optional_str(params, "from"),
                to: optional_str(params, "to"),
                status: optional_str(params, "status").or(Some("open".into())),
                unread_for: optional_str(params, "agent"),
                limit: params
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map(|n| n as usize),
            };
            crate::commands::messages::filtered_json(root, &filter)
        }
        "post" => {
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
        other => bail!("unknown messages action '{other}' (use list or post)"),
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

/// `read_vault`: read a vault file as JSON, or a line slice when `start_line`/`end_line` set.
pub fn read_vault(
    root: &Path,
    target: &str,
    start_line: Option<u64>,
    end_line: Option<u64>,
) -> Result<Value> {
    let path = safe_vault_path(root, target)?;
    let raw = fs::read_to_string(&path)
        .map_err(|e| anyhow!("reading {}: {e}", path.display()))?;

    if start_line.is_some() || end_line.is_some() {
        let lines: Vec<&str> = raw.lines().collect();
        let start = start_line.unwrap_or(1).max(1) as usize;
        let end = end_line.unwrap_or(lines.len() as u64).max(1) as usize;
        let hi = end.min(lines.len());
        let lo = start.saturating_sub(1).min(hi);
        let content = lines[lo..hi].join("\n");
        return Ok(json!({"p": target, "c": content}));
    }

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
        let v = read_vault(&root, "state/active_context.yml", None, None).unwrap();
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

        let after = read_vault(&root, target, None, None).unwrap();
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
        assert_eq!(v["pj"], "knogg");
        assert!(v["nx"].is_array());
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

        let before = read_vault(&root, "plans/roles.yml", None, None).unwrap();
        let propose = handle(
            &root,
            "propose_state_update",
            &json!({"target": "plans/roles.yml",
                    "patch": {"roles": {"reviewer": {"summary": "test"}}},
                    "reason": "waiting on review"}),
        )
        .unwrap();
        let id = propose["proposal_id"].as_str().unwrap().to_string();
        // High-risk target stays pending until audit_commit.
        assert_eq!(propose["status"], "pending");

        let mid = read_vault(&root, "plans/roles.yml", None, None).unwrap();
        assert_eq!(before, mid);

        handle(&root, "audit_commit", &json!({ "id": id })).unwrap();
        let after = read_vault(&root, "plans/roles.yml", None, None).unwrap();
        assert!(after.get("roles").is_some());
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
        assert_eq!(names.len(), 8);
        assert!(names.contains(&"get_active_context"));
        assert!(names.contains(&"messages"));
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
        assert_eq!(parsed["pj"], "knogg");
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
        assert_eq!(ctx["pj"], "knogg");
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
    fn legacy_propose_decision_errors() {
        let root = temp_root("legacydec");
        init(root.to_str().unwrap(), false).unwrap();
        assert!(handle(
            &root,
            "propose_decision",
            &json!({"title": "Use X", "reason": "because"}),
        )
        .is_err());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn messages_post_and_read() {
        let root = temp_root("msg");
        init(root.to_str().unwrap(), false).unwrap();
        handle(
            &root,
            "messages",
            &json!({"action": "post", "from": "cursor", "text": "hi"}),
        )
        .unwrap();
        let v = handle(
            &root,
            "messages",
            &json!({"action": "list", "status": "open"}),
        )
        .unwrap();
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["from"], "cursor");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn low_risk_proposal_autoapplies() {
        let root = temp_root("auto");
        init(root.to_str().unwrap(), false).unwrap();
        let v = handle(
            &root,
            "propose_state_update",
            &json!({"target": "state/active_context.yml",
                    "patch": {"focus": {"status": "done"}},
                    "reason": "r"}),
        )
        .unwrap();
        assert_eq!(v["status"], "applied");
        let ctx = read_vault(&root, "state/active_context.yml", None, None).unwrap();
        assert_eq!(ctx["focus"]["status"], "done");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn read_vault_line_range() {
        let root = temp_root("lines");
        init(root.to_str().unwrap(), false).unwrap();
        let v = handle(
            &root,
            "read_vault",
            &json!({"path": "state/active_context.yml", "start_line": 1, "end_line": 3}),
        )
        .unwrap();
        assert_eq!(v["p"], "state/active_context.yml");
        assert!(v["c"].as_str().unwrap().contains("project:"));
        fs::remove_dir_all(&root).ok();
    }
}
