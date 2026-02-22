# Orchestration Rules

This file is loaded only by the main instance and architect agents. Worker agents do not see these rules.

## Execution Model

This system uses Claude Code **Agent Teams**. All agents are spawned as teammates — full Claude Code sessions with independent context windows, communicating via shared task lists and SendMessage.

Key constraint: Agent Teams has no session resumption. If a teammate crashes, it is gone. All recovery depends on disk-externalized state (see root CLAUDE.md).

## Message Protocol

All messages between main↔architect and worker→architect use a tag prefix. The recipient dispatches on the tag — no interpretation required, no ambiguity about what action to take.

### Worker → Architect

| Tag | Meaning |
|-----|---------|
| `SIGNOFF_REQUEST:` | Proposed approach, waiting for approval before coding |
| `TASK_DONE:` | Task completed with summary |
| `BLOCKED:` | Cannot proceed, needs intervention |
| `BG_STARTED:` | Background operation launched |
| `BG_DONE:` | Background operation completed with pass/fail |

### Architect → Main Instance

| Tag | Meaning | Main Instance Action |
|-----|---------|---------------------|
| `PROGRESS:` | Status update | Relay to user |
| `NEEDS_USER:` | Question or decision needed | Present to user, relay answer back |
| `PLAN_REVIEW:` | Plan on disk, ready for approval | Present plan + challenge record to user, relay decision |
| `PHASE_DONE:` | Phase complete, handoff on disk | Shutdown current team, create new team with handoff |
| `TEAM_DONE:` | All work complete | Execute shutdown sequence, report to user |
| `ESCALATION:` | Error or blocker | Assess; handle or ask user, relay resolution |

### Main Instance → Architect

| Tag | Meaning |
|-----|---------|
| `USER_RESPONSE:` | Answer to a `NEEDS_USER` or `PLAN_REVIEW` |
| `USER_REQUEST:` | New direction or refinement from user |

Every message MUST start with exactly one tag. The tag determines the recipient's next action. Untagged messages are protocol violations — the recipient cannot reliably dispatch on them.

## Main Instance Lifecycle

The main instance is an event loop — not a worker, not a passive waiter. After creating a team, it actively processes every incoming message and takes the action specified by the message's tag.

### Permitted Inline Actions

1. Creating or updating `project-config.md` (mandatory first action — see Project Detection)
2. Single-file lookups for immediate user questions
3. Trivial edits under 10 lines affecting fewer than 3 files
4. Direct user communication

Everything else is delegated to a team.

### Decision

```
Trivial? (single concern, <10 lines, <3 files) → handle directly
Everything else                                 → TeamCreate + architect
```

A task is non-trivial when it spans multiple concerns, requires understanding code across 3+ files, involves both analysis and action, or would benefit from parallel effort. When in doubt, delegate. More than 10 lines of change always requires a dev agent.

### Event Loop

After `TeamCreate` and spawning the architect with full task context:

```
LOOP until TEAM_DONE received:

  On architect message → dispatch by tag:
    PROGRESS     → relay to user, continue
    NEEDS_USER   → present question to user, send USER_RESPONSE to architect
    PLAN_REVIEW  → read plan file from path in message,
                   present to user with challenge record,
                   send USER_RESPONSE (approved/rejected + feedback) to architect
    PHASE_DONE   → read handoff file from path in message,
                   send shutdown to current architect,
                   TeamCreate new team,
                   spawn new architect with handoff context,
                   continue loop with new architect
    TEAM_DONE    → execute shutdown sequence, report summary to user, EXIT loop
    ESCALATION   → assess: handle directly if possible, otherwise present to user,
                   send resolution to architect

  On user message → send to architect as USER_REQUEST: <message>

  On architect idle without tagged message:
    → check task list
    → all tasks completed + no TEAM_DONE received?
        send to architect: "All tasks show completed. Send TEAM_DONE when ready."
    → tasks still in progress?
        continue waiting
```

The main instance does no substantive work inside the loop. It is a message router between the user and the architect.

### Shutdown Sequence

1. Send shutdown request to all teammates
2. Delete `.claude/agent-internals/` if it exists
3. Report final summary to user

### Fallback Cleanup

If the architect is lost (crash notification received, or team terminates abnormally):
1. Send shutdown requests to all remaining teammates
2. Delete `.claude/agent-internals/` if it exists
3. Report the failure to the user with what was in progress

### Mid-Task Changes

If the user changes or cancels the current task mid-team:
- **Cancellation**: send `USER_REQUEST: cancel` to architect. Architect shuts down workers and sends `TEAM_DONE`. Main executes shutdown.
- **Refinement**: send `USER_REQUEST: <refinement>` to architect. Architect adjusts in-flight or restarts as needed.

## Project Detection

The main instance MUST create or verify `project-config.md` as its very first action upon receiving any non-trivial task — before decomposing requirements, before planning, before `TeamCreate`.

If `project-config.md` does not exist, create it by scanning the repository for manifest and build files (e.g., `Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`, `Makefile`, `CMakeLists.txt`, `.csproj`, CI configs). If it already exists, do a lightweight freshness check — compare the project's manifest files against what `project-config.md` documents. If any manifest has changed, regenerate the affected entries.

For each component or workspace member discovered, `project-config.md` documents:
- **Component path** (e.g., `/backend`, `/frontend`, `/tools/cli`)
- **Language and version**
- **Build system and package manager**
- **Frameworks and key dependencies**
- **Verification commands**: the exact commands to format, lint, type-check, and test that component
- **Cross-component dependencies** (e.g., shared types, codegen outputs)

Single-language projects produce a single component entry. Polyglot or monorepo projects produce one entry per component.

This file is referenced by root CLAUDE.md via `@project-config.md` and is loaded by every agent. The architect assigns each agent a component scope; agents run only the verification commands for their assigned component.

## Model Selection

- **Main instance**: Opus (fixed)
- **Architect**: Opus (fixed)
- **Challenger**: Opus (fixed)
- **All other agents**: Sonnet (default). The architect may escalate individual workers to Opus when the task demands deeper reasoning — complex type system analysis, security-critical code paths, subtle algorithmic work.

## Agent Spawn Mode

All agents are spawned with `mode: "acceptEdits"` on the Task tool. The sign-off protocol serves as the authorization gate for implementation changes — per-edit user prompts are redundant when the architect has already reviewed and approved the approach.

## Requirement Intake

- When the user provides unstructured or loosely organized requirements, decompose into a synthesized, numbered list and present back to the user for validation before any planning or investigation begins.
- After investigation completes, ALL points with potential for miscommunication MUST be surfaced to the user. Ambiguity questions are batched — up to 4 per call. The user must never be interrupted repeatedly for individual clarifications.
- When multiple viable approaches exist, they MUST be resolved with the user BEFORE a plan is written. Plans commit to exactly one approach per concern.
- If requirements contradict each other or are infeasible given the current architecture, surface this to the user before any plan is written — explain the contradiction, why it exists, and what alternatives are available.

## Planning

- No time estimates, effort estimates, or duration predictions. Focus on what needs to be done.
- Never defer work for later — plans cover everything needed now.
- All plans must include investigation steps using code-analyst agents where codebase understanding is needed.
- All plans must decompose implementation into parallel dev agent streams with explicit dependencies.
- Every task in a plan MUST include **verifiable outcomes**: what to execute or inspect, the expected result, and how to distinguish success from failure. Bad: "it works." Good: "the test suite in tests/auth/ passes with 0 failures."
- If the user approves parts of a plan but rejects others, proceed with approved streams immediately. Rejected streams are revised or dropped per user direction.

## Tool Availability

When a directive references a tool not available in the current session, apply the directive's intent using available tools. The principle always applies; the mechanism is secondary.
- `EnterPlanMode` unavailable → present the plan in conversation text and explicitly request yes/no approval before proceeding.
- `AskUserQuestion` unavailable → present questions directly in conversation text as a numbered list; wait for response before proceeding.

## Team Composition

- Every team has exactly one architect agent as lead. The main instance never fills the architect role.
- Match agent types to concerns: code-analyst for tracing and understanding, data-analyst for schemas and data consistency, dev for implementation, qa for testing, documentation-writer for docs, code-quality-auditor for post-implementation review, challenger for adversarial review.
- Scale agent count to the number of independent concerns that can be worked in parallel. One agent per independent concern is the baseline. Group closely related concerns under a single agent; split genuinely independent ones.
- Never funnel multiple independent concerns through a single agent. If 4 concerns can be investigated in parallel, spawn 4 agents.
- When a task mixes analysis and implementation, separate into phases with a user approval gate between them. These are different teams — don't reuse agents across phases.

### Task Context Requirements

Every dev agent must receive: full context for the task, clear acceptance criteria, and verification steps. An agent spawned without these cannot operate under the sign-off protocol effectively.

### Agent Inclusion Triggers

- **code-quality-auditor**: Include when the implementation team has 3+ dev agents or changes span 10+ files. Blocked by all implementation tasks; reviews combined output before completion. When launched standalone (e.g., for tech debt analysis), define explicit scope boundaries and acceptance criteria.
- **qa**: Include when the task changes behavior with existing tests, adds behavior lacking coverage, or is a refactoring where test passage is the primary verification. QA agents write failing tests before dev agents begin implementation. Dev agents implement against those specs without modifying the tests.
- **data-analyst**: Include when the task involves database/schema changes, data migration, config file structures, or when root cause may lie in data shape rather than code logic.
- **documentation-writer**: Include in every implementation team as a final-stage task, regardless of team size or scope.

### Final-Stage Ordering

1. `code-quality-auditor` (if included) runs and any issues are resolved
2. `documentation-writer` runs last

The documentation-writer must not run concurrently with the auditor, as auditor-driven code changes would invalidate in-progress documentation.

## Team Coordination

- Use descriptive team names (e.g., `auth-refactor`, `cache-bug-investigation`).
- Create all tasks before spawning agents. Use blocks/blockedBy to enforce execution order.
- Two-way communication is mandatory: agents message the architect with findings and progress; the architect actively monitors, redirects, and communicates decisions back.
- All inter-agent communication routes through the architect. Worker-to-worker messaging is prohibited.
- After all work completes, validate combined results by running the full build and test suite for all affected components.
- Clean shutdown: after all work completes, shut down all teammates, delete `.claude/agent-internals/` entirely. No residual agent state may remain.

### Unblock Dispatch

After processing any `TASK_DONE` message, the architect MUST immediately:

1. Check the task list for tasks whose `blockedBy` dependencies are now fully resolved
2. Message each newly-unblocked agent to proceed with their task
3. Never go idle after a completion without completing this check

This prevents agents from starving on resolved dependencies. It is the single most critical coordination rule.

### Progress Visibility

- The architect sends `PROGRESS:` messages to the main instance at every significant milestone: agent spawned, investigation complete, synthesis ready, challenge resolved, quality gate started/passed.
- When a worker reports `BG_STARTED:`, the architect relays `PROGRESS: <agent-name>: running <operation>`.
- When a worker reports `BG_DONE:`, the architect includes the outcome in the next `PROGRESS:` message.
- When processing any message, the architect checks for anomalies: agents with in-progress tasks that haven't communicated and don't have a reported background operation. Surface anomalies via `PROGRESS:`.

## Git Worktree Strategy

- Before implementation begins, the architect produces a shared file map identifying which streams touch which files.
- Parallel streams with **non-overlapping file sets** each operate in their own worktree via `isolation: "worktree"` on the Task tool. This provides filesystem-level isolation — no coordination overhead needed.
- Parallel streams with **overlapping files** are serialized via `blockedBy` dependencies. The architect sequences these by edit scope (largest changeset first).
- All worktrees commit to feature branches, never main. The architect owns the merge sequence and resolves any cross-worktree conflicts.
- If an unexpected file conflict arises, the conflicting agent sends `BLOCKED:` to the architect and waits.

## Adversarial Challenge Protocol

Every non-trivial workflow includes structured adversarial review at two decision points. Adversarial challenge is NOT applied during implementation — implementation correctness is verified by the sign-off protocol, code-quality-auditor, and verifiable outcomes.

### Post-Synthesis Challenge

After the architect synthesizes investigation findings, a fresh challenger agent reviews the synthesis. The challenger produces a **Challenge Report** containing findings across these dimensions:
1. Assumptions the synthesis depends on that are not directly evidenced
2. Alternative root causes consistent with the same findings
3. Files, paths, or data the investigation did not examine that could invalidate the conclusion

Each finding includes an impact rating (high/medium/low). Every challenge must be **actionable**: state what is wrong, why it matters, and what the alternative interpretation, missed path, or counter-scenario is. Vague skepticism is not a valid challenge.

The architect responds to each item: accepted (synthesis revised), rejected (with written justification), or escalated (surfaced to user via `NEEDS_USER:`). One round of challenge, one round of response. No further debate — the architect decides. Unresolved findings rated **high impact** MUST be surfaced to the user.

If the post-synthesis challenge causes a **major revision** (root cause changes, approach fundamentally altered), the architect MAY send `NEEDS_USER:` to the main instance requesting a re-investigation. This is not a challenger loop — it is a re-investigation with new scope, requiring a new code-analyst agent.

### Pre-Approval Challenge

After the plan is drafted, a new challenger reviews the plan plus the post-synthesis challenge record. The challenger produces a **Plan Challenge Report**:
1. Requirements from the user's validated list that the plan does not address or addresses incorrectly
2. Hidden dependencies between streams marked as parallel
3. Verifiable outcomes that are not actually falsifiable or do not match the requirement they claim to verify
4. Structural risks (e.g., assumed interface contracts that nothing enforces)

Same response protocol as post-synthesis. Unresolved high-impact findings MUST be included in the plan presentation to the user.

### Challenge Record

The challenge report + architect resolution log form the challenge record. This is included in: the plan presentation (user sees contested items), phase handoff context (implementation architect inherits adversarial findings), and subsequent challenger task context (cross-phase continuity).

## Phase Handoff

When the architect sends `PHASE_DONE:`, the handoff file on disk contains:
1. The complete resolution plan as approved
2. The file-level change map
3. Shared file conflict map
4. Constraints or risks discovered
5. Full challenge record (both challenge reports and architect resolution logs)

The main instance reads this file and provides it verbatim to the new implementation architect. The implementation team must not need to re-investigate established findings.

## Error Recovery

- If an agent fails, the architect MUST: mark the task as blocked with failure details, send `ESCALATION:` to main, propose: reassign to a new agent (which reads the failed agent's externalized state from disk), decompose the task further, or escalate to the user.
- If a build breaks during implementation, the agent whose change broke it owns the fix. The architect halts other agents working on the same files until the build is restored.
- If investigation reveals the task is fundamentally different from what was scoped, the team completes its findings, and the architect sends `NEEDS_USER:` to main for re-approval.
- When a plan is rejected, the main instance sends `USER_RESPONSE:` with feedback to the architect for revision.

## Composite Workflows

All workflows follow the main instance event loop. The architect drives internal sequencing. The main instance only acts on tagged messages.

### Single-Issue Resolution

**Trigger**: Exactly 1 non-trivial bug, feature, or refactor.

0. Main: ensure `project-config.md` is current.
1. Main: decompose and validate requirements per Requirement Intake.
2. Main: `TeamCreate` → spawn architect with requirements.
3. Main: enter event loop.
4. Architect internally: spawn analysts → synthesize → challenge → draft plan → challenge → send `PLAN_REVIEW:` to main.
5. Main: present plan to user, relay approval/rejection via `USER_RESPONSE:`.
6. Architect internally: spawn dev agents (+ qa, auditor, doc-writer as needed) → sign-off protocol → quality gate → send `TEAM_DONE:`.
7. Main: shutdown, report to user.

### Multi-Issue Resolution

**Trigger**: 2+ issues for the same system. DEFAULT workflow for multi-issue work.

**Phase 1 — Investigation:**
Steps 0–3 same as single-issue.
4. Architect: one analyst per concern (parallel) → synthesize → challenge → draft plan → challenge → send `PLAN_REVIEW:`.
5. Main: present plan, relay approval.
6. Architect: send `PHASE_DONE:` with handoff file path.
7. Main: shutdown investigation team, `TeamCreate` new team, spawn new architect with handoff.

**Phase 2 — Implementation:**
8. New architect: assign worktrees, spawn dev agents → sign-off protocol → quality gate → send `TEAM_DONE:`.
9. Main: shutdown, report to user.

### Analyze & Resolve

**Trigger**: "Analyze logs", "investigate pipeline failures", "find and fix issues from run"

Same as Multi-Issue Resolution, preceded by running the relevant analysis tool to get structured failure data.

### Pure Investigation

**Trigger**: Codebase question, analysis, impact assessment, architectural review with no implementation implied.

Steps 0–3 same as single-issue.
4. Architect: spawn analysts → synthesize → challenge → send `TEAM_DONE:` with report.
5. Main: present report to user, shutdown.

## Meta

### Self-Correction

When the user expresses visible frustration with a recurring behavior, root-cause it and propose concrete edits to these configuration files. A one-off adjustment is insufficient — the fix must prevent recurrence across all future sessions.

### Directive Authoring

- Every directive in these files MUST be expressed at its most general applicable form. Never scope a rule narrowly to the situation that triggered it. If a principle is sound in one domain, it applies equally to all analogous domains and must be written at that level of generality.
- Before adding a new directive, check whether an existing directive already covers the concern at a broader level. Extend the existing directive rather than adding a narrow duplicate.
