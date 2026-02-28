# Project Standards

@project-config.md

All agents MUST run formatting, linting, and type-checking for their assigned component (defined in `project-config.md`) and achieve zero errors before marking any task complete. For tests: dev and qa agents run only the tests covering code paths their changes or specifications touch. The architect runs the full test suite at the quality gate.

## Investigation & Analysis
- Fix the **ROOT CAUSE**, not the symptom. Every investigation must trace to the actual underlying cause.
- Never introduce indirection (callback chains, nested conditionals, flag-driven branching) when a direct, linear path exists.

## Coding Standards
- When multiple independent conditions exist (platform, mode, format, etc.), resolve each to its own variable before using them. Don't write nested if/else chains that branch on combinations of unrelated conditions — resolve each condition once, then compose the results.
- Before writing a second similar block, identify what varies vs. what is shared. Extract the shared shape and parameterize the variation — prefer driving logic with data (collections, maps, parameters) over duplicating branches.
- Before writing, count the genuinely distinct cases the code must handle. The code should have that many branches — not more. If you find nesting depth or branch count exceeding the number of distinct cases, the structure doesn't match the problem.
- Match the abstraction level, idioms, and conventions of the surrounding code. Don't introduce a novel pattern when the codebase already has a working one for the same concern.
- Prefer the smallest change that fully solves the problem. When a refactor produces a structurally simpler result than layering changes onto existing complexity, the refactor is correct — even if it touches more code. The deciding criterion is final structural complexity, not diff size.
- When adding behavior to an existing operation, extend it — add a parameter, a variant, a config key — rather than duplicating the function or class. One function with a new parameter beats two functions that are 90% identical.
- Don't create abstractions, helpers, or wrapper functions for one-time operations. Three similar lines of code is better than a premature abstraction. Don't add configuration options, extension points, or feature flags for hypothetical future requirements.
- Name variables and functions for what they represent in the domain, not with generic placeholders. `usersByEmail` not `data`. `validateTokenExpiry` not `process`. `retryWithBackoff` not `handler`. If a name requires a comment to explain it, the name is wrong.
- Only validate at system boundaries — user input, external APIs, file I/O. Don't add null checks, try/catch, or fallback defaults for internal code paths where the types already guarantee the shape. Trust the type system and framework guarantees.
- Only modify code necessary for the task. Don't add type annotations, docstrings, or comments to unchanged code. Don't reformat adjacent lines or refactor neighboring functions. Every line in the diff should trace to the task requirements.

## Zero Legacy
No historical references, deprecation markers, `// legacy`, `// old`, commented-out code, or vestigial code paths may remain after a change. When code becomes obsolete as a result of new work, it is fully removed in the same change — not annotated, not commented out, not left for later.

## Compaction Resilience
When compacting, always preserve:
- The full list of modified files
- All verification commands and their expected results
- The current task's acceptance criteria
- Any blocking issues or dependencies on other agents

If context is lost despite preservation, call `orchestrator:recover` to rebuild state from the event store.

## Inter-Agent Communication
All communication between agents routes through the architect. Direct peer-to-peer messaging between worker agents is prohibited. The architect must have visibility into all state and decisions to maintain coordination integrity.

## Documentation
Any change that alters documented behavior, APIs, or configuration must include corresponding documentation updates in the same changeset. Documentation updates ship with the code — never deferred.
