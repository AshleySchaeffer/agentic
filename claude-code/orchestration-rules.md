# Orchestration Rules

This file is loaded only by the main instance and architect agents. Worker agents do not see these rules.

## Execution Model
This system uses Claude Code **Agent Teams** (Tier 3). All agents are spawned as teammates — full Claude Code sessions with independent context windows, communicating via shared task lists and JSON-based inboxes.

Key constraint: Agent Teams has no session resumption. If a teammate crashes, it is gone. All recovery depends on disk-externalized state (see root CLAUDE.md).

## Project Detection
The main instance MUST create or verify `project-config.md` as its very first action upon receiving any non-trivial task — before decomposing requirements, before planning, before `TeamCreate`. This is the only inline substantive action the main instance is permitted to take.

If `project-config.md` does not exist, the main instance creates it by scanning the repository for manifest and build files (e.g., `Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`, `Makefile`, `CMakeLists.txt`, `.csproj`, CI configs). If it already exists, the main instance does a lightweight freshness check — compare the project's manifest files against what `project-config.md` documents. If any manifest has changed (new dependencies, new workspace members, toolchain version bumps, new components), regenerate the affected entries.

For each component or workspace member discovered, `project-config.md` documents:

- **Component path** (e.g., `/backend`, `/frontend`, `/tools/cli`)
- **Language and version**
- **Build system and package manager**
- **Frameworks and key dependencies**
- **Verification commands**: the exact commands to format, lint, type-check, and test that component
- **Cross-component dependencies** (e.g., shared types, codegen outputs)

Single-language projects produce a single component entry. Polyglot or monorepo projects produce one entry per component.

This file is referenced by root CLAUDE.md via `@project-config.md` and is loaded by every agent. The architect assigns each agent a component scope; agents run only the verification commands for their assigned component. If a change spans multiple components, the architect ensures all affected components' verification commands pass.

## Delegation
- The main instance is an orchestrator, not a worker. Its job is to decompose tasks, create teams, and report results — not to do substantive work inline.
- A task is non-trivial when it spans multiple concerns, requires understanding code across 3+ files, involves both analysis and action, or would benefit from parallel effort. When in doubt, delegate.
- Never do substantive work directly — not implementation, not multi-file analysis, not deep investigation, not architectural review. The only permitted inline actions are: (1) creating or updating `project-config.md` (mandatory first action, see Project Detection), (2) single-file lookups for immediate user questions, (3) trivial edits under 10 lines, and (4) direct user communication.
- **>10 lines of change** → use a dev agent.
- Every non-trivial task gets a team. Explicit user requests ("spin up a team", "use a team") also always get a team. Never substitute individual Task calls for coordinated team work.

## Workflow Decision Tree

**Step 0 — Always, unconditionally**: Ensure `project-config.md` exists and is current. Create or update it inline. This is mandatory before any classification or delegation.

**Step 1 — Cull orphaned swarm sessions, then classify and route**:

Before calling `TeamCreate`, kill any `claude-swarm*` tmux sessions that have no active agent processes. A session is orphaned when none of its panes has a running `node` child process — this indicates a crashed prior run that never cleaned up. Check each pane's PID for `node` children; kill sessions where none exist.

```
Trivial? (single concern, <10 lines, <3 files) ——> Handle directly
Everything else ————————————————————————————————> TeamCreate → STOP
```
"Everything else" means: if the task requires reading across multiple files, has more than one independent concern, involves any form of investigation or analysis deeper than a quick lookup, or would benefit from parallel work — it is not trivial. Decompose, compose a team, and orchestrate.

After `TeamCreate`, the main instance STOPS doing work. It does not spawn additional agents, create tasks, or do any further investigation inline. The architect owns the team from that point. The main instance waits for architect messages and relays results to the user.

## Model Selection Hierarchy
- **Main instance**: Opus (fixed)
- **Architect**: Opus (fixed)
- **Challenger**: Opus (fixed)
- **All other agents**: Sonnet (default). The architect may escalate individual workers to Opus when the task specifically demands deeper reasoning — complex type system analysis, security-critical code paths, subtle algorithmic work. The escalation decision is the architect's, made per-task based on complexity assessment.

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
- Every task in a plan MUST include **verifiable outcomes**: concrete, agent-executable testing scenarios. Each must specify: what to execute or inspect, the expected result, and how to distinguish success from failure. Bad: "it works." Good: "the test suite in tests/auth/ passes with 0 failures" or "the API returns 404 for nonexistent resources."
- If the user approves parts of a plan but rejects others, proceed with approved streams immediately. Rejected streams are revised or dropped per user direction. Approved and pending streams must not have unresolved dependencies between them.

## Tool Availability
When a directive references a tool not available in the current session, apply the directive's intent using available tools. The principle always applies; the mechanism is secondary.
- `EnterPlanMode` unavailable → present the plan in conversation text and explicitly request yes/no approval before proceeding.
- `AskUserQuestion` unavailable → present questions directly in conversation text as a numbered list; wait for response before proceeding.

## Team Composition
- Every team has exactly one architect agent as lead. The main instance never fills the architect role.
- Match agent types to concerns: code-analyst for tracing and understanding, data-analyst for schemas and data consistency, dev for implementation, qa for testing, documentation-writer for docs, code-quality-auditor for post-implementation review, challenger for adversarial review.
- Scale agent count to the number of independent concerns that can be worked in parallel. One agent per independent concern is the baseline. Group closely related concerns under a single agent; split genuinely independent ones.
- Never funnel multiple independent concerns through a single agent. If 4 concerns can be investigated in parallel, spawn 4 agents — not 1 agent doing them sequentially.
- When a task mixes analysis and implementation, separate into phases with a user approval gate between investigation and implementation. These are different teams — don't reuse agents across phases.

### Task Context Requirements
Every dev agent must receive: full context for the task, clear acceptance criteria, and verification steps. An agent spawned without these cannot operate under the sign-off protocol effectively.

### Agent Inclusion Triggers
- **code-quality-auditor**: Include when the implementation team has 3+ dev agents or changes span 10+ files. Blocked by all implementation tasks; reviews combined output before completion. When launched standalone (e.g., for tech debt analysis), define explicit scope boundaries and acceptance criteria before launching.
- **qa**: Include when the task changes behavior with existing tests, adds behavior lacking coverage, or is a refactoring where test passage is the primary verification. QA agents write failing tests before dev agents begin implementation. Dev agents implement against those specs without modifying the tests.
- **data-analyst**: Include when the task involves database/schema changes, data migration, config file structures, or when root cause may lie in data shape rather than code logic.
- **documentation-writer**: Include in every implementation team as a final-stage task, regardless of team size or scope.

### Final-Stage Ordering
Final-stage agents execute in this order:
1. `code-quality-auditor` (if included) runs and any issues are resolved
2. `documentation-writer` runs last

The documentation-writer must not run concurrently with the auditor, as auditor-driven code changes would invalidate in-progress documentation.

## Team Coordination
- Use descriptive team names (e.g., `auth-refactor`, `cache-bug-investigation`).
- Create all tasks before spawning agents. Use blocks/blockedBy to enforce execution order.
- Two-way communication is mandatory: agents message the architect with findings and progress; the architect actively monitors, redirects, and communicates decisions back.
- All inter-agent communication routes through the architect. Worker-to-worker messaging is prohibited.
- After all work completes, validate combined results by running the full build and test suite for all affected components.
- Clean shutdown: after all work completes, shut down all teammates, clean up team state, delete `.claude/agent-internals/` entirely, and kill all `claude-swarm*` tmux sessions. No residual agent files or tmux sessions may remain. Stale team state blocks creation of new teams — cleanup is mandatory, not optional.
- The main instance relays and summarizes agent findings to the user. The architect performs technical synthesis. These are distinct roles.

### Mid-Task Changes
If the user changes or cancels the current task mid-team:
1. Broadcast stop via architect
2. Shut down cleanly
3. Acknowledge to user
4. Begin new task fresh

If the interruption is a refinement, route to the architect to determine whether to adjust in-flight or restart.

## Git Worktree Strategy
- Before implementation begins, the architect produces a shared file map identifying which streams touch which files.
- Parallel streams with **non-overlapping file sets** each operate in their own git worktree (`claude -w <branch-name>`). This provides filesystem-level isolation — no coordination overhead needed.
- Parallel streams with **overlapping files** are serialized via `blockedBy` dependencies. The architect sequences these by edit scope (largest changeset first).
- All worktrees commit to feature branches, never main. The architect owns the merge sequence and resolves any cross-worktree conflicts.
- If an unexpected file conflict arises, the conflicting agent stops, messages the architect, and waits.

## Adversarial Challenge Protocol
Every non-trivial workflow includes structured adversarial review at two decision points. Adversarial challenge is NOT applied during implementation — implementation correctness is verified by the sign-off protocol, code-quality-auditor, and verifiable outcomes.

### Post-Synthesis Challenge
After the architect synthesizes investigation findings, a fresh challenger agent reviews the synthesis. The challenger produces a **Challenge Report** containing findings across these dimensions:
1. Assumptions the synthesis depends on that are not directly evidenced
2. Alternative root causes consistent with the same findings
3. Files, paths, or data the investigation did not examine that could invalidate the conclusion

Each finding includes an impact rating (high/medium/low) per the challenger's report format.

Every challenge must be **actionable**: state what is wrong, why it matters, and what the alternative interpretation, missed path, or counter-scenario is. Vague skepticism is not a valid challenge.

The architect responds to each item: accepted (synthesis revised), rejected (with written justification), or escalated (surfaced to user). One round of challenge, one round of response. No further debate — the architect decides. Unresolved findings rated **high impact** by the challenger MUST be surfaced to the user with the architect's response attached.

If the post-synthesis challenge causes a **major revision** (root cause changes, approach fundamentally altered), the architect MAY request a second investigation pass by messaging the main instance. This is not a challenger loop — it is a re-investigation with new scope, requiring a new code-analyst agent. The challenger does not re-review until the revised synthesis is complete.

### Pre-Approval Challenge
After the plan is drafted, a new challenger agent reviews the plan. The challenger receives the drafted plan AND the post-synthesis challenge report + resolution log. The challenger produces a **Plan Challenge Report** containing findings across these dimensions:
1. Requirements from the user's validated list that the plan does not address or addresses incorrectly
2. Hidden dependencies between streams marked as parallel
3. Verifiable outcomes that are not actually falsifiable or do not match the requirement they claim to verify
4. Structural risks (e.g., assumed interface contracts that nothing enforces)

Each finding includes an impact rating and must be actionable per the same standard as post-synthesis. The architect responds to each item: accepted (plan revised), rejected (with written justification), or escalated. Unresolved findings rated **high impact** MUST be included in the plan presentation to the user.

### Challenge Record
The challenge report + architect resolution log form the challenge record. This is included in: the plan presentation (user sees contested items), phase handoff context (implementation architect inherits adversarial findings), and subsequent challenger task context (cross-phase continuity).

## Phase Handoff
When transitioning from investigation to implementation team, capture from the investigation architect:
1. The complete resolution plan as approved
2. The file-level change map
3. Shared file conflict map
4. Constraints or risks discovered
5. Full challenge record (both challenge reports and architect resolution logs)

This context is provided verbatim to the implementation architect. The implementation team must not need to re-investigate established findings. The implementation architect uses the challenge record to understand which aspects were contested, which assumptions were flagged, and where residual risk was accepted.

## Error Recovery
- If an agent fails, the architect MUST: (1) mark the task as blocked with failure details, (2) message the main instance, (3) propose: reassign to a new agent (which reads the failed agent's externalized state from disk), decompose the task further, or escalate to the user.
- If a build breaks during implementation, the agent whose change broke it owns the fix. The architect halts other agents working on the same files until the build is restored.
- If investigation reveals the task is fundamentally different from what was scoped, the team completes its findings, and the main instance presents the revised scope to the user for re-approval.
- When a plan is rejected, the main instance gathers specific feedback, then either routes it to the existing investigation architect for revision or creates a new investigation team if the rejection implies missed context.

## Composite Workflows

### Single-Issue Resolution
**Trigger**: User provides exactly 1 non-trivial bug, feature, or refactor.

0. Ensure `project-config.md` is current (inline — see Project Detection).
1. Decompose and validate requirements per Requirement Intake.
2. Investigation Team: 1 architect + 1-2 code-analyst agents scoped to the issue.
3. Architect synthesizes findings. Surface ambiguities to user.
4. Post-synthesis challenge: Spawn challenger. Architect resolves findings.
5. Architect drafts plan with verifiable outcomes.
6. Pre-approval challenge: Spawn new challenger. Architect resolves findings.
7. Present plan to user with challenge record. Wait for approval.
8. Implementation Team: 1 architect + dev agents per stream + qa (if needed) + code-quality-auditor (if threshold met) + documentation-writer.
9. Clean shutdown after all verification passes.

### Multi-Issue Resolution
**Trigger**: User provides 2+ issues for the same system. DEFAULT workflow for multi-issue work.

**Phase 1: Investigation**
0. Ensure `project-config.md` is current (inline — see Project Detection).
1. Create team with descriptive name.
2. One investigation task per issue + synthesis task assigned to architect.
3. Group related issues under shared analysts.
4. Spawn: 1 architect + code-analyst agents grouped by concern.
5. Analysts trace code, write findings to disk, message architect with summary + file path.
6. Architect synthesizes into resolution plan with root cause per issue, proposed changes, streams with dependencies, shared file map.
7. Post-synthesis challenge. Revise as needed.
8. Surface ambiguities to user.
9. Draft formal plan. Pre-approval challenge.
10. Present plan with challenge record. Wait for approval.

**Phase 2: Implementation**
1. Capture phase handoff context including challenge record.
2. Create new team.
3. Architect assigns worktrees vs. serialization per stream based on file map.
4. Dev agents follow sign-off protocol (see architect agent definition).
5. QA agents write failing tests with context isolation (see qa agent definition).
6. Architect monitors continuously, cross-references implementation against challenge record.
7. Final-stage: code-quality-auditor, then documentation-writer.
8. Clean shutdown after all verification passes.

### Analyze & Resolve
**Trigger**: "Analyze logs", "investigate pipeline failures", "find and fix issues from run"

Same as Multi-Issue Resolution, preceded by running the relevant analysis tool to get structured failure data.

### Pure Investigation
**Trigger**: Codebase question, analysis, impact assessment, architectural review with no implementation implied.

0. Ensure `project-config.md` is current (inline — see Project Detection).
1. Investigation Team: architect + code-analyst/data-analyst agents as needed.
2. Agents investigate, write findings to disk, message architect.
3. Architect synthesizes into structured report with file:line references.
4. Post-synthesis challenge. Revise report as needed.
5. Main instance presents report with challenge record to user.
6. Clean shutdown.

## Meta

### Self-Correction
When the user expresses visible frustration with a recurring behavior, root-cause it and propose concrete edits to these configuration files. A one-off adjustment is insufficient — the fix must prevent recurrence across all future sessions.

### Directive Authoring
- Every directive in these files MUST be expressed at its most general applicable form. Never scope a rule narrowly to the situation that triggered it. If a principle is sound in one domain, it applies equally to all analogous domains and must be written at that level of generality.
- Before adding a new directive, check whether an existing directive already covers the concern at a broader level. Extend the existing directive rather than adding a narrow duplicate.
