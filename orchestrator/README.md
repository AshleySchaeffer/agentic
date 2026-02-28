# Orchestrator

A Claude Code plugin that makes agent team interactions reliable. It intercepts Claude Code's native tools via hooks to persist every message and task event, detect dead agents, rewrite oversized messages to file references, inject unblock notifications and alerts directly into active agents' context windows, and recover state after context compaction. A terminal UI provides real-time visibility into agents, tasks, messages, and alerts. Agents use Claude Code's native tools normally — the orchestrator is transparent infrastructure.

## Prerequisites

- Rust toolchain (stable) — [install via rustup](https://rustup.rs)

## Build

```sh
cd orchestrator
cargo build --release
```

The binary is produced at `orchestrator/target/release/orchestrator`.

## Install

1. Copy the `plugin/` directory to your Claude Code plugins location:
   ```sh
   cp -r orchestrator/plugin ~/.claude/plugins/orchestrator
   ```
2. Ensure the `orchestrator` binary is on your `PATH`:
   ```sh
   cp orchestrator/target/release/orchestrator ~/.local/bin/orchestrator
   # or any directory already on PATH
   ```

## Usage

Start the daemon (run once per project session):
```sh
orchestrator bootstrap
```

Open the terminal UI (run in a separate terminal or tmux pane):
```sh
orchestrator tui
```

Check daemon status:
```sh
orchestrator status
```

Stop the daemon:
```sh
orchestrator stop
```

## Configuration

Place `.claude/orchestrator/config.json` in your project root to override defaults. All fields are optional.

| Field | Default | Description |
|-------|---------|-------------|
| `socket_path` | `.claude/orchestrator/daemon.sock` | Unix domain socket path |
| `db_path` | `.claude/orchestrator/orchestrator.db` | SQLite database path |
| `docs_dir` | `.claude/orchestrator/docs` | Directory for offloaded large message content |
| `message_size_threshold` | `2048` | Bytes above which message content is offloaded to a file |
| `unanswered_timeout_secs` | `300` | Seconds before a delivered-but-unanswered message is flagged |
| `stall_timeout_secs` | `600` | Seconds before an in-progress task with no activity is flagged as stalled |
| `automation_interval_secs` | `30` | Seconds between automation engine ticks |

Example:
```json
{
  "message_size_threshold": 4096,
  "unanswered_timeout_secs": 600,
  "stall_timeout_secs": 900
}
```
