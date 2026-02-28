# Reimplementation Design

## What we're replacing and why

The current system models a software engineering organization. There's an architect who leads, analysts who investigate, a challenger who reviews, developers who implement, QA who writes tests, an auditor who checks quality, and a documentation writer who updates docs. Communication routes through the architect via tagged messages. State is externalized to disk so agents can recover from crashes. The orchestration rules run to 340 lines and define protocols for every interaction pattern.

This worked, to a degree. But it hit a ceiling. The fundamental issue is that it asks LLMs to do what they're worst at — reliably execute multi-step stateful coordination protocols — in order to enable what they're best at — reasoning about code. The majority of every agent's prompt is protocol overhead, not domain instruction. The architect spends most of its context tracking who said what, dispatching unblocks, relaying messages, and following a 15-step process. When it drops a step, which it inevitably does, teams hang silently.

The user experience suffers because the system makes too many decisions autonomously. By the time a plan reaches the user, eight steps of investigation, synthesis, challenge, and resolution have already happened. The user reviews a fait accompli rather than participating in the design.

The agent roles themselves mirror a human org chart. Analyst, architect, QA, documentation writer — these are human job titles, not LLM optimization points. An LLM doesn't write better tests because you called it a QA specialist. It writes better tests because you gave it clear requirements and focused context. The current system pays the coordination cost of role specialization — per-role protocols, interface contracts, sequencing rules — without evidence that the role boundary itself adds capability beyond what task scoping provides. Every role boundary is a message hop, a potential failure, and information loss.

## The core insight

LLMs are single reasoning processes. They read context, think, produce output, receive feedback, and refine. They're excellent at deep reasoning on focused problems, generating alternatives, reviewing critically, and producing code against clear specs. They're terrible at reliably executing stateful protocols, long-running coordination across many concurrent actors, and self-awareness of context loss.

Coordination between agents isn't eliminated by reducing their count — parallel implementers still need reliable message delivery, dependency tracking, and recovery from context loss. What changes is where coordination lives. The current system puts it in prompts: 340 lines of protocol rules that every agent must execute reliably as multi-step stateful procedures. The reimplementation moves it to infrastructure that delivers clear, single-step signals. An agent that must "check blockedBy, look up the assignee, format a message, send it, verify delivery, update the task" will drop steps. An agent that receives "Task X is now unblocked, proceed" and acts on it will succeed reliably. The orchestrator handles the multi-step procedures; agents respond to their outputs.

## Three roles

Every agent in the system is one of three types. These aren't human job titles — they're modes of LLM usage optimized for what LLMs actually do well.

**Planner.** A single agent that investigates the codebase, reasons about the problem, and works interactively with the user to design a solution. It does its own code reading. It doesn't delegate investigation to analyst agents and wait for findings — it reads the code itself, because that's one fewer hop of information loss. When the codebase is large enough that parallel investigation of independent areas would help, it spawns lightweight exploration subtasks, but these are a tool the planner uses, not a separate role with its own protocol. The planner's primary loop is: read code, identify a decision point, present options to the user, incorporate the user's choice, continue. Planning is a conversation, not a pipeline.

**Reviewer.** An adversarial agent with a fresh context window. It exists because confirmation bias is real — the planner, having built up a line of reasoning, will naturally favor it. The reviewer sees the same evidence without the planner's commitment to a conclusion. It operates at two moments: before options reach the user (catching biased framing, missing alternatives, omitted tradeoffs) and on the assembled plan before execution (catching structural issues, hidden dependencies, missing requirements). One agent, two touchpoints. One round of review at each, no debate loops.

**Implementer.** An agent that writes code, tests, and documentation against a clear spec derived from the plan. Self-contained — it gets everything it needs to execute from the plan, does its work, runs verification, and delivers. For independent concerns, multiple implementers run in parallel. An implementer scoped to "auth backend" and one scoped to "frontend validation" are effectively specialized — they have focused context for their area. The difference from named roles is that they share a common prompt shape and don't need per-role coordination protocols. The specialization comes from task scoping, not persona definitions with their own interface contracts and interaction rules. When the task is large enough to warrant a final review pass over combined output, that's a reviewer task.

Everything the current system has beyond these three — code-analyst, data-analyst, QA, documentation-writer, code-quality-auditor — collapses into task-scoped instances of these roles. Analysis is something the planner does (or delegates to exploration subtasks it controls). Testing is something an implementer does against acceptance criteria defined during planning — and when adversarial independence matters, a separate implementer instance writes tests against the spec without seeing the implementation, providing the same blind-spot coverage that a named QA role aimed for. Documentation updates are part of implementation. Quality review is what the reviewer does. The roles that disappear aren't the capabilities they provided — those remain. What disappears is the per-role protocol overhead: the interface contracts, the message formats, the sequencing rules, the ceremony.

## The planning flow

The current planning flow is a pipeline: investigate, synthesize, challenge, draft plan, challenge again, present to user. Each step is autonomous. The user is downstream of all decisions.

The new flow is a conversation, but not a drip-feed. The planner investigates, reasons, and groups related decisions into coherent clusters at natural decision boundaries. A cluster might be "data model choices" or "API design tradeoffs" — a set of interrelated decisions that make sense together and where seeing the relationships matters for choosing well. The planner presents each cluster with genuine options and honest tradeoffs, and the user resolves the cluster before the planner continues to the next.

Before each cluster reaches the user, the reviewer filters it — catching anchoring bias, missing options, misleadingly described tradeoffs, and decision interdependencies the planner may not have surfaced. The user never sees unvetted framing, but also never reviews 15 individual choices or a wall of text summarizing eight steps of autonomous work. Three to five decision clusters is typical for a non-trivial task.

When all clusters are resolved, the plan exists as the natural consequence of the user's choices assembled into an execution order. The reviewer checks the assembled plan for structural issues that are hard to see when decisions were made individually — hidden dependencies between parallel streams, requirements that fell through the cracks, choices made in one cluster that contradict choices in another, verification steps that don't actually verify what they claim to. This is the final gate before execution.

This means the user shapes the design through a small number of meaningful interactions, rather than being absent throughout and confronted at the end, or interrupted at every micro-decision.

## The orchestrator

Coordination moves to a standalone infrastructure layer — the Rust plugin described in IDEA.md. This adds infrastructure complexity, and that's a deliberate trade: infrastructure complexity can be tested, debugged, and observed deterministically. Prompt complexity fails silently when an agent drops a protocol step. Infrastructure either works or raises an error.

The orchestrator intercepts only orchestration-relevant tool calls via hooks: SendMessage, TaskCreate, TaskUpdate, TaskGet, TaskList, and session lifecycle events. It does not hook Read, Grep, Glob, Write, Edit, Bash, or any other tool. This keeps the hook surface small and adds zero latency to the high-frequency tools agents use for actual work. The orchestrator tracks what agents are communicating and how tasks are progressing — it doesn't monitor everything agents do.

From these intercepts, the orchestrator provides: persistent message delivery with dead-recipient detection, a task dependency graph with automatic unblock detection, compaction recovery from the event store, and alerts for stalls and failures. Agents use SendMessage, TaskCreate, TaskUpdate normally and receive the benefits transparently.

Context injection via `additionalContext` is subject to a strict budget. Injections only occur when there's an actionable orchestration event — an unblocked task, a dead teammate, recovery context after compaction. Routine status is never injected; the TUI serves that purpose for the human. When multiple alerts exist, they're batched and prioritized rather than injected individually. The budget caps total injected context to prevent orchestration overhead from crowding out the agent's actual work.

When the orchestrator detects a compaction event — a gap in an agent's expected activity pattern following context that was previously continuous — it automatically injects recovery context on the agent's next orchestration-relevant tool call. The agent doesn't need to know it was compacted. The explicit `orchestrator:recover` MCP tool exists as a manual fallback for cases the heuristic misses, not as the primary recovery mechanism.

The TUI provides visibility that the current system achieves through PROGRESS messages relayed through the architect to the main instance to the user. Agent status, task graphs, message flow, and alerts are visible directly in a terminal pane. The user sees what's happening without depending on any agent to remember to report it.

## What this means for the repository

The current repository contains agent definitions, orchestration rules, global configuration, and slash commands — all in markdown, all loaded as prompt context. The reimplementation splits this into two concerns.

The orchestrator is a Rust project. It owns everything that is currently prompt-based protocol: message delivery, task tracking, health monitoring, state persistence, unblock dispatch, context recovery. It ships independently and works with any agent definitions.

The agent definitions become minimal. Three files — planner, reviewer, implementer — each containing a role description and domain instructions. No protocol overhead, no state externalization rules, no message tag tables. The orchestration rules file either disappears entirely (its concerns are in infrastructure) or shrinks to a short document covering the planning conversation flow and the reviewer's touchpoints.

The global CLAUDE.md retains coding standards and project-level conventions. These are domain instructions that belong in prompts. Everything else that's currently in it — inter-agent communication rules, compaction resilience, long-running operation protocols — is deleted because the orchestrator handles those concerns.

## What this means in practice

A task goes from the user to the planner. The planner reads code, thinks, and presents decision clusters to the user at natural boundaries. The user shapes the design through a small number of meaningful choices. The reviewer ensures the user sees honest options and catches structural issues in the final plan. Implementers — scoped to independent concerns, effectively specialized by task rather than persona — execute the plan in parallel. The orchestrator keeps communication reliable, dependencies tracked, and failures visible.

Agent prompts contain role descriptions and domain instructions. Coordination protocols, state externalization rules, compaction resilience, message tag tables, unblock dispatch procedures — all of this moves to infrastructure that handles it reliably without depending on LLM protocol adherence. The system has three types of agent that reason and one infrastructure layer that coordinates, instead of eight agent types that try to both reason and coordinate themselves through 340 lines of prompt rules.
