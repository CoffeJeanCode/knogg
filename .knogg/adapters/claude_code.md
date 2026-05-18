<!-- generated-by: knogg -->
# Handoff → Claude Code (architect)

**Role: architect** — plan, review, and record decisions. Do not apply proposals or ship code directly.

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
| Implement / refactor src/ | Cursor (builder) |
| make test, release, knogg CLI | OpenCode (executor) |
| Plan, review, ADRs | You (architect) |
