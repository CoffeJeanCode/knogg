# Contributing to knogg

Thank you for your interest in contributing! This document covers how to get started, coding standards, and the review process.

## Table of Contents

- [Development Setup](#development-setup)
- [Running Tests](#running-tests)
- [Code Style](#code-style)
- [Making Changes](#making-changes)
- [Pull Request Process](#pull-request-process)
- [Commit Messages](#commit-messages)
- [Reporting Issues](#reporting-issues)

## Development Setup

### Prerequisites

- **Docker** (Compose v2) — the only runtime dependency
- No local Rust toolchain required for building or testing

### Quick Start

```bash
# Clone the repository
git clone https://github.com/<owner>/knogg.git
cd knogg

# Enter the dev container
make dev

# Or run commands directly via Docker
make test          # run the test suite
make release       # build Unix + Windows binaries into ./dist
```

### Project Structure

```
knogg/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── cli.rs               # clap subcommand definitions
│   ├── mcp/mod.rs           # JSON-RPC stdio server
│   ├── commands/            # one file per subcommand
│   └── core/                # vault, config, vaultio (shared utilities)
├── .knogg/                  # vault (project knowledge + state)
├── Cargo.toml
├── docker-compose.yml
├── Dockerfile.dev
├── Makefile
└── README.md
```

## Running Tests

```bash
# Full test suite (Docker)
make test

# Inside dev container
cargo test
cargo test -- --nocapture   # with output
cargo test module_name      # single module
```

All tests must pass before submitting a PR. New features should include unit tests.

## Code Style

### Rust Conventions

- **Edition**: Rust 2021
- **Formatting**: `max_width = 100` (see `rustfmt.toml`)
- **Module docs**: Every `src/commands/*.rs` file must start with a `//!` module doc comment
- **Error handling**: Use `anyhow::Result` for application code, `thiserror` for library errors
- **Naming**: snake_case for modules/files, PascalCase for types, UPPER_SNAKE_CASE for constants

### Linting

```bash
# Inside dev container
cargo clippy -- -D warnings
cargo fmt --check
```

### Vault Files

The `.knogg/` directory is the project's context store. Key conventions:

- All writes go through `atomic_write` (temp file + rename)
- Acquire `VaultLock` before mutating state
- Never write `state/` files directly — use `propose_state_update` for staged changes
- Path traversal (`..`) is rejected everywhere

## Making Changes

1. **Pick an open task**: Check `plans/master_plan.yml` for tasks with `status: todo`
2. **Claim it**: `knogg task claim <id> --agent <your-name>`
3. **Make changes**: Edit the files listed in the task
4. **Test**: `make test` must pass
5. **Release task**: `knogg task release <id> --agent <your-name>`

### Adding a New Command

1. Define the subcommand and action enum in `src/cli.rs`
2. Create `src/commands/<name>.rs` with a `//!` module doc
3. Register the module in `src/commands/mod.rs`
4. Wire the match arm in `src/main.rs`
5. Add MCP tool registration in `src/mcp/mod.rs` (if applicable)
6. Write unit tests in the same file under `#[cfg(test)]`

## Pull Request Process

1. **Branch**: Create a feature branch from `main`
2. **Scope**: Each PR should address a single task or cohesive set of changes
3. **Tests**: All existing tests pass; new tests for new functionality
4. **Linting**: `cargo clippy -- -D warnings` and `cargo fmt` are clean
5. **Description**: Explain what changed and why (link to the task if applicable)
6. **Review**: At least one approval required before merge

### PR Checklist

- [ ] Tests pass (`make test`)
- [ ] No clippy warnings (`cargo clippy -- -D warnings`)
- [ ] Code is formatted (`cargo fmt`)
- [ ] CHANGELOG.md updated (Unreleased section)
- [ ] Documentation updated if behavior changed

## Commit Messages

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]
```

**Types**: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`

**Examples**:

```
feat(proposal): add gc command with status/keep/project filters
fix(release): atomic rename for binary copy to avoid ETXTBSY
docs(readme): add MCP tool reference table
test(plan): add claim/release roundtrip test
```

Keep the subject line under 72 characters. Use the body to explain *why*, not *what*.

## Reporting Issues

- **Bug reports**: Use the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md)
- **Feature requests**: Use the [feature request template](.github/ISSUE_TEMPLATE/feature_request.md)
- **Security vulnerabilities**: See [SECURITY.md](SECURITY.md)

Include: knogg version, Docker version, OS, and steps to reproduce (for bugs).

## License

By contributing, you agree that your contributions will be licensed under the [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0).
