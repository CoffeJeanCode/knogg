# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.3.3] — 2026-05-28

### Added
- **`knogg link <ide>`** — auto-configure the knogg MCP server for Cursor or Claude (writes `.cursor/mcp.json` / `~/.claude.json`)
- **MCP Resources** — `resources/list` and `resources/read` expose `knogg://core/architecture` and `knogg://state/active_context` as native MCP resources
- **MCP Prompts** — `prompts/list` and `prompts/get` serve the `knogg-task` prompt with live vault state injected (current focus, next actions, agent-ownership warnings)
- **FatProposal** — `propose_state_update` now accepts optional `adr_proposal` (inline ADR) and `message_to_human` fields; a single MCP call can atomically stage a state patch, record a decision, and leave a human-readable note
- **`knogg triage`** — interactive approve/reject loop for pending proposals; applying a proposal with an inline ADR writes the decision log atomically under one vault lock
- **Schema auto-migration** — `read_yaml_typed<T>` transparently patches old vault YAMLs with missing fields using deep-merge defaults; patched files are written back silently so old vaults require no manual migration
- `knogg initialize` now advertises `resources` and `prompts` capabilities in the MCP handshake

### Changed
- `knogg watch` replaces the removed `sync` command with `brief refresh` on state changes
- Event hooks: `"sync"` action in existing `hooks.yml` files now triggers a brief refresh (backward-compatible)
- `active_context.yml` `focus` fields (`stage`, `task`, `status`) all have Serde defaults — missing fields no longer cause deserialization failures on old vaults
- `Focus.owner` field added (optional, skip-serialized when empty) to support agent-ownership tracking in prompts

### Removed
- `knogg sync` command — replaced by `brief refresh` triggered automatically by `watch` and hooks

## [1.1.0] — 2026-05-19

### Added
- **Knogg Mesh — Federation Layer** — cross-project agent communication via TCP hub
- `knogg hub` — central router for multi-project mesh (`knogg hub --port 5050`)
- `query_mesh` MCP tool — agents query other projects' vaults through the hub
- `MeshClient` — TCP client with register/query/list-peers; auto-connects via `KNOGG_HUB_URL`
- Hub service in `docker-compose.yml` for easy local mesh testing

### Changed
- MCP server dispatches mesh queries to internal `call_tool_pub` for vault reads

## [1.0.1] — 2026-05-18

### Added
- **v1 MCP surface (ADR-0010)** — 8 tools; `get_active_context` returns brief, scope, handoff, and inbox in one terse payload
- **`messages` MCP tool** — `action=list|post` replaces separate get/post/ack tools
- **Risk-tiered proposals (ADR-0011)** — low-risk `active_context`/`brief` patches auto-apply when `proposals.autoapply_low = true`; pending proposals on the same target are auto-superseded
- **`read_vault` line ranges** — `start_line` / `end_line` for partial reads
- Stale open messages auto-close after 30 days

### Changed
- Dropped legacy MCP tools (`get_brief`, `get_next_actions`, role/decision mutators, etc.) — use consolidated context or CLI
- `make check` runs `fmt-check`, `lint`, and `test`

## [1.2.0] — 2026-05-19

### Added
- **P2P Mesh — Direct Peering** — `knogg serve --port <PORT>` async TCP JSON-RPC server
- **Declarative Peering** — `knogg.toml [mesh]` section with `listen_port` + static `[mesh.peers]` table
- **Connection Pool** — auto-reconnect on peer failure; resilient mesh topology
- **query_peer MCP tool** — federated cross-vault queries via P2P pool
- **subscribe_to_task MCP tool** — subscribe to task-done events from connected peers
- **Event subscriptions** — `state set --status done` emits task-done events to subscribers
- **`knogg unlock`** — manually clear stale lock files (global + per-file)
- **`knogg gc`** — reclaim disk space: purge old backups + terminal proposals
- **Stale lock reclamation** — lock files with dead PIDs are auto-reclaimed after 30s timeout
- **Granular lock metadata** — lock files carry PID, owner, timestamp, intent (JSON)
- **Schema migrations** — transparent vault YAML upgrades on read
- Hub service in `docker-compose.yml` with exposed port 5050

### Changed
- Lock timeout increased from 5s to 15s to accommodate network-backed vault access
- Vault files now carry a `version` field for forward-compatible schema upgrades
- `knogg watch` also starts P2P peers from `knogg.toml [mesh]` on boot

### Fixed
- Lock reclamation prevents stale lock hang when `knogg` crashes

## [1.0.0] — 2026-05-16

### Added
- `knogg init` — create vault tree with core docs, plans, adapters
- `knogg status` — print project/stage/task/status
- `knogg doctor` — integrity diagnostics (exit non-zero on errors)
- `knogg handoff --to <agent>` — render compact handoff prompts (Cursor, Claude, Codex)
- `knogg sync` — generate tool config files from templates (human-file protection)
- `knogg state set/add-next/clear-next` — safe active context edits (lock + atomic rename)
- `knogg decision add` — append ADR entries with incremental IDs
- `knogg proposal list/show/apply/reject` — staged proposal lifecycle
- `knogg mcp` — JSON-RPC over stdio server
- `knogg watch` — file watcher with debounce, reactive sync
- Global lock (`.knogg/.lock`) with RAII + 5s timeout
- Atomic writes (temp file + rename) — crash-safe
- Backup system for `init --force` / `sync --force`
- Path boundary checks (`..` rejected, MCP rejects absolute paths)
- Docker-first development (no local Rust required)
- Windows cross-compilation via mingw-w64
- Minijinja handoff templates per agent
- Agent registry with per-agent MCP config sync
- Role system (architect, builder, executor)
- Tool registry mapping templates to outputs
- 93 unit tests covering vault, MCP, commands, and safety guarantees
