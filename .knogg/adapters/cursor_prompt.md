<!-- generated-by: knogg -->
# Handoff → Cursor (builder)

**Role: builder** — implement and refactor Rust in `src/`. Hand off test/release runs to OpenCode; design questions to Claude.

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

## Delegation

| Need | Agent |
|------|--------|
| Plan, review, ADRs | Claude (architect) |
| make test, release, knogg doctor | OpenCode (executor) |
| Code in src/ | You (builder) |
