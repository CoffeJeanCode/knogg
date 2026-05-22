# Command Reference

Every command accepts `--path <dir>` (defaults to `./.knogg` or `knogg.toml`).

## Summary

| Command | Purpose |
|---------|---------|
| `knogg init [--force]` | Create / regenerate the knogg tree |
| `knogg status` | Print project / stage / task / status |
| `knogg doctor` | Diagnose integrity (exit ≠ 0 on error) |
| `knogg handoff --to <agent>` | Render handoff prompt |
| `knogg sync [--force] [--dry-run]` | Generate tool config files from templates |
| `knogg state set` | Update active context |
| `knogg state add-next` / `clear-next` | Manage next actions |
| `knogg decision add` / `set-status` | Append ADRs / update ADR status |
| `knogg proposal list/show/apply/reject/gc` | Manage staged proposals |
| `knogg messages list` / `ack` | Agent message log |
| `knogg agents list/sync/set-role/…` | Agent registry management |
| `knogg role set/list/show/remove` | Role specifications |
| `knogg hooks list/run/enable/disable` | Event-driven hooks |
| `knogg brief refresh/show/doctor` | Compact project brief |
| `knogg task list/claim/release` | Partitioned task management |
| `knogg hub [--port]` | Start federation hub |
| `knogg serve [--port]` | Start P2P TCP JSON-RPC server |
| `knogg unlock --all/--file` | Clear stale vault lock files |
| `knogg gc [--dry-run]` | Reclaim disk space |
| `knogg style list/show/doctor` | Coding conventions |
| `knogg mcp` | Run MCP server (JSON-RPC over stdio) |
| `knogg watch` | Re-run `sync` when active context changes |

---

## `knogg init [--force]`

Create the knogg tree and base files.

```bash
knogg init                 # first time
knogg init --force         # regenerate (backs up changed files)
```

---

## `knogg status`

Print active context: project, stage, task, status.

```bash
knogg status
# Project: knogg
# Stage:   Stage 8 — Dynamic workflows
# Task:    OpenCode: agent capabilities + registry (8D)
# Status:  in_progress
```

---

## `knogg doctor`

Diagnose knogg integrity. Reports `[ok]` / `[warn]` / `[error]`, prints `Result: healthy|unhealthy`, exits non-zero on errors.

---

## `knogg handoff --to <agent> [--print] [--save <file>]`

Render compact handoff prompt. Injects only `project.name`, `focus.*`, `constraints`, `next_actions`, `handoff.summary` — never the full vault.

- Agents: `cursor`, `claude`, `codex`
- `--print` and `--save` can be combined

```bash
knogg handoff --to cursor --print
knogg handoff --to claude --save .handoff/claude.md
```

---

## `knogg sync [--force] [--dry-run]`

Render templates from `plans/tool_registry.yml` to outputs (`.cursorrules`, `.claude/context.md`, `AGENTS.md`).

```bash
knogg sync --dry-run       # preview
knogg sync                 # apply
knogg sync --force         # overwrite human-owned files (with backup)
```

---

## `knogg state …`

Edit `state/active_context.yml` safely (lock + atomic write).

```bash
knogg state set --stage auth --task "Add login" --status in_progress
knogg state set --status done
knogg state add-next "Update billing page"
knogg state clear-next
```

`--status` values: `todo`, `in_progress`, `blocked`, `done`.

---

## `knogg decision …`

### `decision add`

Append ADR entry with incremental id (`ADR-0001`, …).

```bash
knogg decision add \
  --title "Use staged proposals for agent changes" \
  --reason "Agents should propose before applying state mutations" \
  --status accepted
```

`--status` values: `proposed`, `accepted`, `rejected`, `superseded`.

### `decision set-status`

Update status on existing ADR(s). Accepts multiple ids.

```bash
knogg decision set-status ADR-0005 ADR-0006 --status accepted
```

---

## `knogg proposal …`

Manage staged state-change proposals. Supports multiple ids for show/apply/reject.

```bash
knogg proposal list                          # PROP-0001  pending  state/active_context.yml
knogg proposal show PROP-0001 PROP-0002
knogg proposal apply PROP-0001 PROP-0002     # best-effort batch — not atomic
knogg proposal reject PROP-0001
knogg proposal gc                            # remove terminal proposals
knogg proposal gc --status applied --keep 5  # keep latest 5 per status
```

---

## `knogg messages …`

Agent message log for structured coordination.

```bash
knogg messages list                          # all messages
knogg messages list --from cursor --limit 10
knogg messages list --unread-for opencode
knogg messages ack MSG-0001 MSG-0002 --by opencode
```

---

## `knogg agents …`

Manage agent workspace configuration (MCP configs per agent).

```bash
knogg agents list
knogg agents doctor
knogg agents inspect
knogg agents sync --dry-run
knogg agents set-role cursor builder
knogg agents enable opencode
knogg agents disable codex
knogg agents enable-mcp cursor knogg
knogg agents disable-mcp cursor knogg
```

---

## `knogg role …`

Manage agent role specifications.

```bash
knogg role set builder --summary "Edits Rust code" --responsibility "Implement features"
knogg role list
knogg role show builder
knogg role remove builder
```

---

## `knogg hooks …`

Manage event-driven hooks.

```bash
knogg hooks list
knogg hooks doctor
knogg hooks run after_state_change
knogg hooks enable after_state_change
knogg hooks disable after_state_change
```

---

## `knogg brief …`

Manage compact project brief.

```bash
knogg brief refresh
knogg brief show
knogg brief doctor
```

---

## `knogg task …`

Manage partitioned tasks from `plans/master_plan.yml`.

```bash
knogg task list
knogg task claim 7A --agent cursor
knogg task release 7A --agent cursor
```

---

## `knogg hub [--port]`

Start federation hub — TCP router for cross-project agent communication.

```bash
knogg hub                  # port 5050
knogg hub --port 6060
```

See [mesh.md](mesh.md) for full setup.

---

## `knogg serve [--port]`

Start read-only TCP JSON-RPC server for P2P peering.

```bash
knogg serve                 # port 5051
knogg serve --port 6060
```

Auto-connects to peers declared in `knogg.toml [mesh.peers]`. See [mesh.md](mesh.md).

---

## `knogg unlock`

Clear stale vault lock files. Locks auto-reclaim after 30s — manual unlock only needed in edge cases.

```bash
knogg unlock --all
knogg unlock --file state/active_context.yml
```

---

## `knogg gc`

Reclaim disk space: purge old backups + terminal proposals.

```bash
knogg gc                    # dry-run by default
knogg gc --dry-run
```

Rules:
- `.knogg/backups/<stamp>/` older than 7 days → removed
- `.knogg/state/proposals/<id>.yml` with `applied` or `rejected` status → deleted

---

## `knogg style …`

Manage coding conventions from `core/style_guides.yml`.

```bash
knogg style list
knogg style show --lang rust
knogg style doctor
```

---

## `knogg mcp`

Run MCP server (JSON-RPC over stdio). See [mcp.md](mcp.md).

---

## `knogg watch`

Watch `state/active_context.yml` and re-run `sync` on change (300–500 ms debounce).

```bash
# Terminal 1
knogg watch
# Terminal 2 — state change triggers sync automatically
knogg state set --status done
```
