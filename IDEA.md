# Claude Code Agent Orchestrator — Spec

A Rust plugin for Claude Code that makes agent team interactions reliable. Ships as a Claude Code plugin (hooks + MCP server) with a terminal UI. Does not interfere with existing agent definitions or Claude Code configuration.

The orchestrator is topology-agnostic. It does not assume an architect, a hub-and-spoke model, or any particular team structure. It works with flat teams, hierarchical teams, or any arrangement Claude Code supports. It provides infrastructure primitives — agents and users drive the workflow.

## Problems Solved

1. **Agent-to-agent delivery** — Claude Code's SendMessage is fire-and-forget. Messages to dead agents vanish silently. The orchestrator guarantees persistence, tracks delivery, detects dead recipients, and notifies senders of failures.

2. **Task tracking between agents** — Task dependencies exist as metadata, but nothing acts on them. When a task completes, nobody automatically tells the blocked agents they're unblocked. The orchestrator builds a dependency graph from Task tool intercepts and surfaces unblock events to the agents that need to act.

3. **Large message context duplication** — A 200-line findings document sent via SendMessage lands in the recipient's context window in full, whether the recipient needs all of it or not. The orchestrator transparently rewrites large messages to file references, giving the recipient control over what enters its context.

4. **Context compaction** — When Claude Code compacts an agent's context, the agent loses conversational history: what it was working on, what messages it received, what depends on it. The orchestrator's event store persists everything independently of any agent's context window. Recovery reconstructs state from the store.

## Integration Model

Agents use Claude Code's native tools normally — SendMessage, TaskCreate, TaskUpdate, Read, Write, Edit, Bash. The orchestrator intercepts these via hooks, persists events, and adds reliability guarantees transparently. Agents do not need to know the orchestrator exists.

The orchestrator uses two Claude Code hook capabilities:

- **`updatedInput`** — PreToolUse hooks can modify tool parameters before execution. This enables transparent message rewriting (large content → file reference) without the agent's awareness.
- **`additionalContext`** — PreToolUse hooks can inject context into the agent's conversation before a tool executes. This enables the orchestrator to push alerts, unblock notifications, and state reminders to active agents without MCP tool calls.

A minimal set of MCP tools are exposed for capabilities that hooks cannot provide (state recovery for compacted agents). These are the only tools agents interact with directly.

## Scope

### 1. Message Delivery

PreToolUse hook intercepts every SendMessage.

**Persistence**: message is recorded in the event store before the tool executes — sender, recipient, timestamp, payload hash, delivery status. PostToolUse confirms delivery.

**Dead recipient detection**: the hook checks recipient liveness (from SessionEnd tracking). If the recipient is dead, the message is blocked (exit 2) and the sender is told the recipient is unavailable.

**Large message handling**: if the message payload exceeds a configurable size threshold, the hook writes the full content to a file under `.claude/orchestrator/docs/` and rewrites the message content to a short reference via `updatedInput`. The recipient receives the reference and reads the file if/when it needs the detail. Small messages pass through untouched. The threshold is configurable (default ~50 lines / 2-3KB). The agent never knows which path was taken.

**One-sided conversation detection**: a message sent with no response and no tool activity from the recipient within a configurable threshold is flagged as an alert.

### 2. Task Tracking

PreToolUse hook intercepts every TaskCreate, TaskUpdate, TaskGet, and TaskList call. Extracts task ID, assignee, status, `blockedBy` list, description. Builds a dependency DAG in the event store.

**Unblock detection**: when a task's status changes to completed, the automation engine walks the dependency graph and identifies tasks whose blockers are now fully resolved.

**Unblock dispatch**: the orchestrator injects `additionalContext` into the completing agent's tool call: "Tasks X, Y are now unblocked. Notify agents A, B to proceed." The completing agent sends the messages via SendMessage, which wakes up the idle agents. If the completing agent exits without dispatching, the notification is injected into the next active agent's tool call as a fallback. If all agents are idle, the alert surfaces in the TUI for the human.

**Stall detection**: a timer checks for in-progress tasks with no hook activity (no tool calls from the assigned agent) past a configurable threshold. Stalled tasks are flagged as alerts.

### 3. Event Store

Append-only event log in SQLite. Every hook invocation produces an event: tool name, parameters, agent ID, session ID, timestamp, associated task (if detectable). All other state is derived from this log.

The event store is the source of truth for:
- Agent lifecycle (alive, dead, idle, active)
- Message history and delivery status
- Task dependency graph and status
- Activity timelines per agent
- Alert conditions

The store replaces file-based state externalization. Agents do not need to write coordination state to disk — the orchestrator captures it automatically through hook interception. The `.claude/agent-internals/` convention and all associated prompt rules become unnecessary.

Large message content is the one exception: content exceeding the size threshold is written to `.claude/orchestrator/docs/` as files. The event store records the file path, not the content.

### 4. Context Injection

The orchestrator uses `additionalContext` on PreToolUse hooks to push information to active agents without requiring MCP tool calls or polling.

**Alerts**: stalled tasks, dead agents, undelivered messages, unanswered messages, unblocked tasks awaiting dispatch — all injected into the relevant agent's next tool call. The relevant agent is determined by the event graph: the agent that sent a message to a dead recipient, the agent whose dependency just resolved, the agent that spawned a stalled worker.

**Team state after compaction**: if an agent has been compacted (heuristic: gap in expected activity pattern, or agent explicitly calls recover), a lightweight state summary can be injected via `additionalContext` on subsequent tool calls.

**Injection strategy**: only inject when there's an actionable item. Routine status does not get injected — the TUI serves that purpose for the human. This keeps context overhead minimal.

### 5. Compaction Recovery

When an agent's context is compacted by Claude Code, it loses conversational history. The agent calls `orchestrator:recover` and the orchestrator rebuilds essential context from the event store:

- Current task with description and acceptance criteria
- Recent inbound messages (including any unanswered ones the agent needs to respond to)
- Recent outbound messages
- Files changed (from Write/Edit hook events)
- Blocking issues and pending dependencies
- Active alerts relevant to this agent

One line in agent prompts: "If you've lost context, call `orchestrator:recover`." This replaces all compaction resilience instructions, state externalization rules, and file path preservation requirements.

### 6. Alerts

The daemon monitors for conditions requiring attention:

- **Dead agent** — session ended unexpectedly (SessionEnd hook with in-progress task)
- **Stalled task** — in-progress task with no agent activity past threshold
- **Undelivered message** — message to dead recipient (blocked at send time)
- **Unanswered message** — message delivered with no response past threshold
- **Unblocked task awaiting dispatch** — dependency resolved but assignee not yet notified
- **Detached agent** — agent alive with no messages sent to or from any other agent past threshold

Alerts surface in two places:

1. **TUI** — highlighted in the alerts panel, always visible to the human
2. **Agent context** — injected via `additionalContext` into the relevant agent's next tool call

The TUI is the fallback for all alerts. If no agent is active to receive an injection, the human sees it and can intervene.

### 7. TUI

Terminal UI built with ratatui. Runs in a separate terminal or tmux pane. Reads from the SQLite event store, subscribes to new events via unix socket from the daemon.

Panels:

- **Agents** — name, status (alive / dead / idle / active), current task, time since last activity, alert indicators
- **Tasks** — dependency graph, status (pending / blocked / in-progress / done / stalled), assignee, unblock history
- **Messages** — chronological log, filterable by agent, shows sender / recipient / timestamp / delivery status / response status. Large messages show the file path.
- **Alerts** — active alerts with severity, affected agent/task, time detected

Drill-down: select any agent, task, or message to see full detail — related events from the store, file change history, message thread.

All views are derived from the event store. The TUI renders data, it owns nothing.

## MCP Tools

Minimal surface. Agents use Claude Code's native tools for all normal operations. MCP tools cover only what hooks cannot provide.

| Tool | Purpose |
|------|---------|
| `orchestrator:recover` | Reconstruct state for a compacted or respawned agent. Returns current task, recent messages, files changed, blocking issues, pending dependencies. |

`orchestrator:status` is a candidate for inclusion — an explicit team health snapshot the agent can query on demand. But `additionalContext` injection covers most cases. Include if testing shows agents need to pull state rather than having it pushed.

## Architecture

```
Claude Code (native agent teams)
  │
  │  hooks (PreToolUse, PostToolUse, SessionStart, SessionEnd)
  │  stdin JSON per event
  │
  ▼
Hook handler (short-lived process)
  │
  │  unix socket
  │
  ▼
Orchestrator daemon (long-lived)
  ├── Event store (SQLite)
  ├── Automation engine (unblock detection, stall detection, alert management)
  ├── MCP server (recover tool)
  └── TUI renderer (ratatui, subscribes to event stream)
```

The hook handler is invoked by Claude Code per tool event. It reads the event from stdin, forwards it to the daemon via unix socket, receives the response (updatedInput, additionalContext, or block decision), writes it to stdout, and exits. The handler must be fast — it's in the critical path of every tool call.

The daemon is long-lived. It owns the event store, runs automation rules on a timer and on event ingestion, serves the MCP tool, and pushes events to the TUI.

## Hook Intercepts

| Hook | Tool | Action |
|------|------|--------|
| PreToolUse | SendMessage | Persist message. Check recipient liveness — block if dead. Check content size — rewrite via `updatedInput` if over threshold. Inject pending alerts via `additionalContext`. |
| PostToolUse | SendMessage | Confirm delivery in event store. |
| PreToolUse | TaskCreate | Record task in dependency graph. |
| PreToolUse | TaskUpdate | Record status change. If completing: detect unblocks, inject dispatch instructions via `additionalContext`. |
| PreToolUse | TaskGet, TaskList | Inject pending alerts via `additionalContext` (lightweight — only when actionable items exist). |
| PostToolUse | Write, Edit | Record file change event (agent, file path, timestamp). |
| PostToolUse | Bash | Record activity (for stall detection timers). |
| PreToolUse | Read, Grep, Glob | Record activity (stall detection). Inject pending alerts via `additionalContext` if any exist. |
| | SessionStart | Register agent: ID, session ID, name, timestamp, spawner. |
| | SessionEnd | Mark agent dead. Flag in-progress tasks. Generate alerts for pending messages and waiting agents. |

## Crate Structure

```
orchestrator/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── cli.rs                  # orchestrator hook / orchestrator daemon / orchestrator tui
│   │
│   ├── hook.rs                 # stdin JSON → event, forward to daemon via socket, return response
│   │
│   ├── daemon/
│   │   ├── mod.rs              # event loop, socket listener
│   │   ├── automation.rs       # unblock detection, stall detection, dispatch logic
│   │   └── alerts.rs           # alert conditions, state, severity
│   │
│   ├── store/
│   │   ├── mod.rs
│   │   ├── db.rs               # SQLite via rusqlite
│   │   ├── events.rs           # append-only event log
│   │   ├── agents.rs           # agent lifecycle (derived view)
│   │   ├── tasks.rs            # task graph and status (derived view)
│   │   └── messages.rs         # message log and delivery status (derived view)
│   │
│   ├── mcp/
│   │   ├── mod.rs
│   │   ├── server.rs           # MCP protocol implementation
│   │   └── tools.rs            # recover tool handler
│   │
│   └── tui/
│       ├── mod.rs
│       ├── app.rs              # app state, event loop, key bindings
│       └── views/
│           ├── agents.rs
│           ├── tasks.rs
│           ├── messages.rs
│           ├── alerts.rs
│           └── detail.rs       # drill-down view
│
├── migrations/
│   └── 001_initial.sql
│
└── plugin/
    ├── plugin.json
    └── hooks.json
```

## Plugin Packaging

Ships as a Claude Code plugin. The plugin contains hook definitions pointing at the orchestrator binary and an MCP server registration.

No agent definitions. No skills. No slash commands. No CLAUDE.md modifications. The plugin is pure infrastructure.

## Guarantee Levels

| Concern | Mechanism | Guarantee |
|---------|-----------|-----------|
| Message persistence | PostToolUse on SendMessage → event store | Hard — every sent message is recorded |
| Dead recipient detection | PreToolUse on SendMessage → liveness check | Hard — hook fires before send, blocks if dead |
| Large message offload | PreToolUse on SendMessage → updatedInput | Hard — hook fires before send, rewrites transparently |
| Compaction recovery | Event store + orchestrator:recover | Hard — data persists independently of agent context |
| Agent death detection | SessionEnd hook | Hard — detection is immediate |
| Stall detection | Timer + event store activity query | Hard — detection is automatic |
| Unblock dispatch | additionalContext on completing agent | Soft — relies on an active agent to send the notification. Fallback: next active agent, then TUI for human. |
| Unanswered message detection | Timer + event store message query | Hard — detection is automatic |
| Alert delivery to agents | additionalContext injection | Soft — requires the agent to make a tool call. TUI fallback for human. |

## What This Removes From Agent Prompts

With the orchestrator handling delivery, tracking, compaction, and alerts:

- State externalization rules and `.claude/agent-internals/` convention — gone
- Compaction resilience instructions — replaced by one line: "call `orchestrator:recover` if you've lost context"
- Unblock dispatch protocol — automated
- Message tagging protocol — can simplify (orchestrator tracks message semantics structurally)
- Progress visibility protocol — TUI replaces manual status reporting
- Disaster recovery procedures — event store + recover tool

Agent prompt files contain only: role description, domain instructions, tool access list.

## Open Questions

1. **MCP tool call timeouts** — Can MCP tool calls block for extended periods? If yes, an `orchestrator:wait()` tool could provide hard-guaranteed delivery to idle agents (agent blocks on wait, orchestrator responds when a message arrives). This would close the soft guarantee on unblock dispatch. Needs testing.

2. **`orchestrator:status`** — Should this be an explicit MCP tool, or is `additionalContext` injection sufficient for all agent-facing observability? Include if agents need to pull state on demand rather than having it pushed.

3. **Compaction detection** — Can the orchestrator detect when an agent has been compacted (via activity pattern gaps or hook metadata), or must the agent self-detect and call recover? If detectable, the orchestrator could auto-inject recovery context via `additionalContext`.

4. **Hook handler latency** — The hook handler is in the critical path of every tool call. Profile the unix socket round-trip to the daemon. If latency is problematic for high-frequency tools (Read, Grep), consider skipping interception for tools where the orchestrator only needs activity timestamps (record in the handler directly, skip the daemon round-trip).
