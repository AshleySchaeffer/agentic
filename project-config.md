# Project Configuration

## Component: agentic (root)

- **Component path**: `/`
- **Language**: Markdown (agent definitions, orchestration rules, documentation)
- **Build system**: None — configuration-only repository
- **Frameworks and key dependencies**: Claude Code Agent Teams
- **Verification commands**:
  - Format: N/A
  - Lint: N/A
  - Type-check: N/A
  - Test: N/A
- **Cross-component dependencies**: None

## Component: orchestrator

- **Component path**: `/orchestrator`
- **Language**: Rust (edition 2021)
- **Build system**: Cargo
- **Frameworks and key dependencies**: tokio (async runtime), rusqlite (bundled SQLite), serde + serde_json (serialization), ratatui + crossterm (TUI), clap (CLI), tracing (logging), sha2 (hashing)
- **Verification commands**:
  - Format: `cd orchestrator && cargo fmt --check`
  - Lint: `cd orchestrator && cargo clippy -- -D warnings`
  - Type-check: `cd orchestrator && cargo check`
  - Test: `cd orchestrator && cargo test`
- **Cross-component dependencies**: None (standalone plugin)
