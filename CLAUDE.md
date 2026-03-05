# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

**Agentic**  - a Claude Code multi-agent configuration. Two agents (dev on Sonnet, reviewer on Opus), five hooks compiled into one binary, and an architect protocol installed as a global CLAUDE.md. See README.md for rationale and design decisions.

## Build & Install

```bash
cargo build --release                  # Build the binary
./target/release/agentic install       # Install to ~/.claude/ and ~/.local/bin/
./target/release/agentic uninstall     # Remove all installed artifacts
```

Rust edition 2024. No test suite  - verification is manual (`cargo build --release` must succeed, then test install/uninstall round-trip).

Debug hooks: `AGENTIC_DEBUG=1 echo '{"hook_event_name":"...","tool_name":"...","cwd":"/tmp"}' | ./target/release/agentic`

## Architecture

Single binary (`src/main.rs`) serves four roles via CLI dispatch:

1. **Hook handler** (no subcommand)  - reads JSON from stdin, dispatches by `(hook_event_name, tool_name)` tuple to one of five handlers
2. **`install`**  - embeds all config files at compile time via `include_str!`, writes them to `~/.claude/`, copies self to `~/.local/bin/agentic`, merges hook entries into `~/.claude/settings.json`
3. **`uninstall`**  - reverse of install, preserving non-agentic user hooks
4. **`refresh`**  - prints create/refresh instructions for project-config.md

### Embedded content → install targets

| Source file | Installed to | Purpose |
|---|---|---|
| `architect.md` | `~/.claude/CLAUDE.md` | Global architect protocol |
| `coding-standards.md` | `~/.claude/coding-standards.md` | Progressive disclosure from CLAUDE.md |
| `agents/dev.md` | `~/.claude/agents/dev.md` | Dev agent prompt |
| `agents/reviewer.md` | `~/.claude/agents/reviewer.md` | Reviewer agent prompt |
| `planning-protocol.md` | (injected at runtime via hook 4) | Never written to disk |

### Hook handlers

| Handler | Trigger | Behavior |
|---|---|---|
| `message_transform` | PreToolUse/SendMessage | Offloads messages >4KB (>2KB if code-heavy) to `.claude/messages/` |
| `agent_accept_edits` | PreToolUse/Agent | Forces `acceptEdits` permission mode |
| `planning_protocol` | PreToolUse/EnterPlanMode | Injects planning protocol as `additionalContext` |
| `post_git_write` | PostToolUse/Bash | Detects `git commit`/`git merge`/`git pull`, re-injects project-config.md content |
| `session_start` | SessionStart | Ensures `@project-config.md` in project CLAUDE.md, bootstraps project-config.md if missing |

### Key design constraints

- Install must be idempotent and preserve existing user hooks in `settings.json`
- Uninstall removes only agentic-owned entries (matched by `"command": "agentic"`)
- `LEGACY_REFS` array tracks filenames from prior versions for cleanup on install/uninstall
- `planning-protocol.md` is never written to disk  - it exists only as a compiled-in string injected via hook context
- `ensure_config_ref` helper is shared between `session_start` and `post_git_write`  - ensures `@project-config.md` reference in the project's CLAUDE.md

@project-config.md
