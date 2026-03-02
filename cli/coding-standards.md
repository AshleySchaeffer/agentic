# Coding Standards

Full standards referenced from CLAUDE.md. These apply to all agents and the main session.

## Investigation & Analysis

- Fix the **root cause**, not the symptom. Every investigation must trace to the actual underlying cause.
- Never introduce indirection (callback chains, nested conditionals, flag-driven branching) when a direct, linear path exists.

## Code Structure

- When multiple independent conditions exist (platform, mode, format, etc.), resolve each to its own variable before using them. Don't nest if/else on combinations of unrelated conditions.
- Before writing a second similar block, identify what varies vs. what is shared. Extract the shared shape and parameterize the variation - prefer data-driven logic over duplicating branches.
- Before writing, count the genuinely distinct cases the code must handle. The code should have that many branches - not more.
- Match the abstraction level, idioms, and conventions of the surrounding code. Don't introduce a novel pattern when the codebase already has a working one.
- When adding behavior to an existing operation, extend it - add a parameter, a variant, a config key - rather than duplicating the function or class.
- Don't create abstractions, helpers, or wrapper functions for one-time operations. Three similar lines of code is better than a premature abstraction. Don't add configuration options, extension points, or feature flags for hypothetical future requirements.

## Naming

- Name variables and functions for what they represent in the domain, not with generic placeholders. `usersByEmail` not `data`. `validateTokenExpiry` not `process`. `retryWithBackoff` not `handler`. If a name requires a comment to explain it, the name is wrong.

## Validation & Error Handling

- Only validate at system boundaries - user input, external APIs, file I/O. Don't add null checks, try/catch, or fallback defaults for internal code paths where the types already guarantee the shape. Trust the type system and framework guarantees.

## Diff Discipline

- Only modify code necessary for the task. Don't add type annotations, docstrings, or comments to unchanged code. Don't reformat adjacent lines or refactor neighboring functions. Every line in the diff should trace to the task requirements.

## Zero Legacy

No historical references, deprecation markers, `// legacy`, `// old`, commented-out code, or vestigial code paths may remain after a change. When code becomes obsolete as a result of new work, it is fully removed in the same change - not annotated, not commented out, not left for later.

## Documentation

Any change that alters documented behavior, APIs, or configuration must include corresponding documentation updates in the same changeset.
