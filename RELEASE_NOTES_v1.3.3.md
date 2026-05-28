# knogg v1.3.3 — MCP-Native Hybrid

## Highlights

### `knogg link` — one-command IDE setup
```
knogg link cursor   # writes .cursor/mcp.json
knogg link claude   # writes ~/.claude.json
```
Merges into an existing config file rather than overwriting it.

### MCP Resources & Prompts
knogg now advertises `resources` and `prompts` capabilities in the MCP handshake.

**Resources** (readable by any MCP client):
- `knogg://core/architecture` — architecture YAML as plain text
- `knogg://state/active_context` — live active context

**Prompts**:
- `knogg-task` — injects live vault state (current focus, next actions) and warns when an agent tries to claim a task already owned by another agent

### FatProposal — atomic agent transactions
`propose_state_update` now accepts two optional fields alongside the existing patch:

```json
{
  "target": "state/active_context.yml",
  "patch": { "focus": { "status": "done" } },
  "adr_proposal": { "title": "Switch to FatProposal", "reason": "Atomic transactions reduce race conditions" },
  "message_to_human": "Finished. Please review the ADR before merging."
}
```

One MCP call → state patch + decision log entry + human message, all atomic under a single vault lock.

### `knogg triage` — interactive proposal review
```
knogg triage
```
Walks through all pending proposals one by one. Applying a proposal that carries an inline ADR writes the decision to `decision_log.yml` atomically in the same lock acquisition.

### Schema auto-migration
Old vault YAMLs with missing or wrong-typed fields are patched transparently on first read — no `knogg init` required after upgrade. The corrected file is written back silently.

## Breaking changes
- `knogg sync` is removed. If you call it in scripts, replace with `knogg brief refresh`.
- Existing `hooks.yml` files that reference the `"sync"` action continue to work (they now trigger `brief refresh`).

## Installation
```bash
# macOS / Linux (replace VERSION and ARCH as needed)
curl -fsSL https://github.com/jeanpierre/knogg/releases/download/v1.3.3/knogg-x86_64-unknown-linux-musl.tar.gz | tar xz
sudo mv knogg /usr/local/bin/

# Re-link your IDE after upgrading
knogg link cursor
knogg link claude
```
