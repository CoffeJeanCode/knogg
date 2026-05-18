<!-- generated-by: knogg -->
# Agent Guide — knogg (Codex / CLI)

Project: {{ project.name }}
Stage: {{ focus.stage }}
Task: {{ focus.task }}
Status: {{ focus.status }}

## Constraints
{% for c in constraints %}- {{ c }}
{% endfor %}
## Next Actions
{% for a in next_actions %}- {{ a }}
{% endfor %}
## Summary
{{ handoff.summary }}

## Commands

| Action | Command |
|--------|---------|
| Build release binary | `make release` → `./dist/knogg` |
| Run tests | `make test` or `docker compose run --rm dev cargo test` |
| Run CLI (wrapper) | `./knogg <cmd>` |
| Vault status | `./knogg status` |
| Integrity | `./knogg doctor` and `./knogg agents doctor` |
| Sync agent files | `./knogg sync --dry-run` then `./knogg sync` |
| Compact brief | `./knogg brief show` |
| MCP server | `./knogg mcp` |

## Structure

```
knogg/                    # repo root
├── src/                  # Rust CLI (cli, commands, core, mcp)
├── .knogg/               # vault (context broker data)
│   ├── core/             # index, architecture, style_guides
│   ├── state/            # active_context, brief, decisions, proposals
│   ├── plans/            # master_plan, registries, roles, hooks
│   └── adapters/         # handoff templates
├── dist/knogg            # release binary (after make release)
├── knogg.toml            # vault path + generated marker
├── knogg                 # wrapper script (dist or Docker)
├── AGENTS.md             # this file (Codex instructions, synced)
├── .cursorrules          # Cursor handoff (synced)
└── .claude/context.md    # Claude handoff (synced)
```

## Standards

- **Rust 2021** in `src/`; format with `cargo fmt`, test with `cargo test` in Docker.
- **Vault writes** only through knogg (lock + atomic rename); agents use `propose_state_update`.
- **Errors**: `anyhow::Result` at boundaries; validate `focus.status` and reject `..` paths.
- **Tests**: colocated `#[cfg(test)]`; temp dirs under `std::env::temp_dir()`.
- **Scope**: prefer focused diffs; do not edit generated outputs without `sync --force`.

## MCP

knogg MCP runs over stdio: `./knogg mcp` (or absolute path to `./dist/knogg`).

Start with `get_active_context` or `get_brief`. To change focus, use `propose_state_update` — never write `.knogg/state/` directly. Humans apply with `./knogg proposal apply PROP-NNNN`.
