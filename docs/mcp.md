# MCP Server

`knogg mcp` speaks **JSON-RPC over stdio** — one JSON request per line in, one JSON response per line out. No HTTP/SSE.

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"get_active_context","params":{}}' \
  | ./knogg mcp
```

## Available Tools

| Tool | Params | Returns |
|------|--------|---------|
| `tools/list` | — | List of available tool names |
| `get_active_context` | `{}` | `state/active_context.yml` as JSON |
| `get_brief` | `{}` | Compact project brief |
| `get_next_actions` | `{}` | Current next actions |
| `get_allowed_scope` | `{agent?}` | Capability-aware scope for an agent |
| `read_vault` | `{path}` | Vault YAML file as JSON (path-boundary checked) |
| `list_vault` | `{include_proposals?}` | Safe relative paths; hides `backups/`, `.lock`, temp files |
| `search_vault` | `{query}` | Search vault files for text (case-insensitive) |
| `get_tool_registry` | `{}` | Registry mappings — never template contents |
| `list_proposals` | `{}` | Staged proposals and their status |
| `propose_state_update` | `{target, patch, reason}` | Creates **pending** proposal; does **not** mutate state |
| `propose_decision` | `{title, reason, scope?, status?}` | Append ADR entry to decision log |
| `audit_commit` | `{id}` | Apply staged proposal by id (re-audits first) |
| `post_message` | `{from, text, to?, reply_to?}` | Post to agent message log |
| `get_messages` | `{from?, to?, limit?, status?, unread_for?}` | Read agent messages |
| `ack_message` | `{id, by}` | Mark message read/acked |
| `get_agent_handoff` | `{agent}` | Rendered handoff prompt for an agent |
| `get_agent_role` | `{agent}` | Role spec assigned to agent |
| `set_agent_role` | `{agent, role}` | Assign role to agent |
| `list_roles` | `{}` | List agent roles |
| `get_role` | `{name}` | Show role by name |
| `set_role` | `{name, summary, responsibilities?, constraints?}` | Create or replace role |
| `get_current_decisions` | `{}` | Recent decisions from brief |
| `get_style_guides` | `{}` | Style guides as JSON |
| `query_mesh` | `{target_project, query, args}` | Query another project via hub |
| `query_peer` | `{peer, method, params}` | Query named P2P peer directly |
| `subscribe_to_task` | `{peer, task_id, from}` | Subscribe to task-done events from peer |

---

## Example: Propose a State Change

```json
{"jsonrpc":"2.0","id":1,"method":"propose_state_update","params":{
  "target":"state/active_context.yml",
  "patch":{"focus":{"status":"in_progress"}},
  "reason":"Move to frontend UI work"}}
```

Human then applies:

```bash
knogg proposal list
knogg proposal apply PROP-0001
```

---

## Registering with Agents

Use an **absolute path** to the binary. Build first: `make release` → `./dist/knogg`.

**Cursor** — `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "knogg": {
      "command": "/ABS/PATH/knogg/dist/knogg",
      "args": ["mcp"]
    }
  }
}
```

**Claude Code** — project `.mcp.json`:

```json
{
  "mcpServers": {
    "knogg": { "command": "/ABS/PATH/knogg/dist/knogg", "args": ["mcp"] }
  }
}
```

**Codex CLI** — `~/.codex/config.toml`:

```toml
[mcp_servers.knogg]
command = "/ABS/PATH/knogg/dist/knogg"
args = ["mcp"]
```

> Restart the agent after editing config. Confirm with a `tools/list` call.

---

## Known Limitation

MCP transport is **stdio only** — no HTTP, SSE, or Streamable HTTP.
