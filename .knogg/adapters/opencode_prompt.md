<!-- generated-by: knogg -->
# Agent Guide — knogg (OpenCode / executor)

**Role: executor** — run builds, tests, and knogg CLI; report results. Defer design and large refactors to architect (Claude) or builder (Cursor).

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

## Commands (primary)

| Action | Command |
|--------|---------|
| Run tests | `make test` |
| Build release | `make release` |
| Vault status / doctor | `./knogg status` / `./knogg doctor` |
| Agent configs | `./knogg agents doctor` / `./knogg agents sync` |
| Sync handoffs | `./knogg sync` |
| Brief | `./knogg brief show` / `./knogg brief refresh` |
| MCP | `./knogg mcp` |

## Delegation

| Need | Agent |
|------|--------|
| Plan, review, ADRs | Claude (architect) |
| Edit Rust in src/ | Cursor (builder) |
| CLI / make / docker runs | You (executor) |

## MCP

Use `get_active_context` or `get_brief` first. Stage focus changes with `propose_state_update`; humans run `./knogg proposal apply PROP-NNNN`.
