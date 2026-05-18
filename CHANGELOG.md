# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `knogg task` subcommand — claim/release partitioned tasks from `master_plan.yml`
- `knogg style` subcommand — list/show/doctor for coding conventions
- `knogg decision set-status` — update ADR status in bulk (variadic ids)
- `knogg proposal apply/reject/show` — variadic ids for batch operations
- `knogg proposal gc` — remove terminal proposals with `--status`, `--keep`, `--project` filters
- `knogg messages ack` — variadic ids for batch acknowledgment
- `knogg agents enable/disable` — toggle agent availability
- `knogg agents enable-mcp/disable-mcp` — attach/detach MCP servers per agent
- `knogg brief refresh/show/doctor` — compact project brief with content-hash freshness
- `knogg hooks` — event-driven hook management (list/doctor/run/enable/disable)
- Capability-aware `get_allowed_scope` MCP tool — returns denied files for open tasks owned by other agents
- Style guide enforcement in `doctor` — module doc checks, optional `cargo fmt --check`
- `init --prompt` — print recommended agent setup prompt
- `init --agents-md` — write `AGENTS.md` guide during vault init
- OpenCode agent adapter (`opencode_prompt.md`)
- `clippy.toml` and `rustfmt.toml` configuration files

### Changed
- MCP server now supports full `initialize` handshake + `tools/call` envelope (backward-compatible with legacy direct methods)
- `doctor` flags pending proposals by default (`--pending-proposals` flag)
- `handoff` auto-fills `handoff.summary` in active context when empty (`--fill-summary`)
- README restructured with TOC, feature table, MCP tool reference, and workflows

### Fixed
- `make release` — atomic rename for binary copy (avoids ETXTBSY on recursive Docker use)

## [0.1.0] — 2026-05-16

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
