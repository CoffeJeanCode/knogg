# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 1.0.x   | :white_check_mark: |
| < 1.0   | :x:                |

## Reporting a Vulnerability

We take security seriously. If you discover a vulnerability in knogg, please:

1. **Do not** open a public issue
2. Email the maintainers with a description of the issue
3. Include steps to reproduce if possible
4. Allow reasonable time for a fix before any public disclosure

We will acknowledge receipt within 48 hours and provide a timeline for a fix.

## Security Model

knogg is designed with the following security guarantees:

### Vault Integrity

- **Global lock**: Every write acquires `.knogg/.lock` (RAII, 5s timeout). No deadlocks, no concurrent writes.
- **Atomic writes**: Content goes to a temp file in the same directory, then `rename(2)` over the target. A crash never leaves a partial file.
- **Backups**: `init --force` and `sync --force` back up changed files before overwriting.

### Path Safety

- Path traversal (`..`) is rejected in all CLI commands and MCP tools
- MCP additionally rejects absolute paths and anything escaping the vault root
- All vault paths are resolved relative to the vault root

### Agent Isolation

- **Staged proposals**: AI agents cannot mutate state directly. Changes are proposed (`propose_state_update`) and require explicit human approval (`knogg proposal apply`).
- **Human files respected**: `knogg sync` never overwrites a file without the generated-by marker unless `--force` is given.
- **Capability-aware scope**: `get_allowed_scope` returns `denied_files` for paths currently being edited by other agents.

### MCP Transport

- stdio JSON-RPC only — no HTTP/SSE exposure
- No network listeners, no remote access
- All data stays local to the project directory

## Known Limitations

- The MCP `initialize` handshake is implemented but the server does not validate protocol version compatibility
- Lock timeout (5s) may be insufficient for very large vaults under heavy concurrent access
- `.knogg/.lock` is advisory — a crashed process may leave a stale lock file (manually remove if needed)
