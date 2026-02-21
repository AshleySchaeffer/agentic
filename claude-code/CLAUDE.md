# Project Standards

@project-config.md

All agents MUST run formatting, linting, and type-checking for their assigned component (defined in `project-config.md`) and achieve zero errors before marking any task complete. For tests: dev and qa agents run only the tests covering code paths their changes or specifications touch — identify affected test files or modules and invoke the test command scoped to those paths. The architect runs the full test suite at the quality gate; full suite runs are not part of per-task agent verification.

## Investigation & Analysis
- Fix the **ROOT CAUSE**, not the symptom. Every investigation must trace to the actual underlying cause.
- Never introduce indirection (callback chains, nested conditionals, flag-driven branching) when a direct, linear path exists.

## Coding Standards
- Isolate each independent axis of variation into its own resolved binding. Compose resolved values rather than branching on every combination of orthogonal concerns.
- Before writing a second similar block, identify what varies vs. what is shared. Extract the shared shape and parameterize the variation — prefer driving logic with data (collections, maps, parameters) over duplicating branches.
- After drafting a change, verify the code's structure mirrors the problem's structure. If nesting depth or branch count exceeds the number of genuinely distinct cases, restructure.
- Match the abstraction level, idioms, and conventions of the surrounding code. Don't introduce a novel pattern when the codebase already has a working one for the same concern.
- Prefer the smallest change that fully solves the problem. When a refactor produces a structurally simpler result than layering changes onto existing complexity, the refactor is correct — even if it touches more code. The deciding criterion is final structural complexity, not diff size.
- When new behavior adds an axis to an existing operation, extend the existing structure with that axis rather than duplicating the structure.

## Zero Legacy
No historical references, deprecation markers, `// legacy`, `// old`, commented-out code, or vestigial code paths may remain after a change. When code becomes obsolete as a result of new work, it is fully removed in the same change — not annotated, not commented out, not left for later.

## State Externalization
All agents MUST write findings, progress, and intermediate state to disk before messaging other agents. The filesystem is the source of truth — conversation context is volatile.

All agent-internal state is written under `.claude/agent-internals/` in the project root. No agent-produced files are written anywhere else.
- Investigation findings → `.claude/agent-internals/findings/<agent-name>-<topic>.md`
- Implementation progress → `.claude/agent-internals/progress/<agent-name>.md`
- Plans and decisions → `.claude/agent-internals/plans/`
- Challenge records → `.claude/agent-internals/challenges/`
- Audit reports → `.claude/agent-internals/audits/`

On clean shutdown, the architect deletes `.claude/agent-internals/` entirely. No agent-produced state files may remain after work is complete.

## Compaction Resilience
When compacting, always preserve:
- The full list of modified files
- All verification commands and their expected results
- The current task's acceptance criteria
- Any blocking issues or dependencies on other agents
- File paths to externalized state

## Inter-Agent Communication
All communication between agents routes through the architect. Direct peer-to-peer messaging between worker agents is prohibited. The architect must have visibility into all state and decisions to maintain coordination integrity.

## Disaster Recovery
If a teammate is lost, a replacement agent reads its disk-externalized state and continues from there. No additional checkpointing beyond normal state externalization is required.

## Documentation
Any change that alters documented behavior, APIs, or configuration must include corresponding documentation updates in the same changeset. Documentation updates ship with the code — never deferred.
