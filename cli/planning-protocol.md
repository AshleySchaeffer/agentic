<planning-protocol>
1. **Classify**: does this extend an existing module, pattern, or convention? If yes → pattern-match. If it needs a new abstraction, data model, API shape, or architectural boundary → novel.
2. **Surface decisions via AskUserQuestion** — 2-3 options with pros/cons and a recommendation per decision point. Batch up to 4 questions per call.
   - **Pattern-match** (extends existing): scope and design decisions only. Make technical implementation calls autonomously — the codebase already answers "how."
   - **Novel** (new abstraction): both design and technical implementation decisions. The user decides how it is built, not just what.
3. **Iterate**: when the user selects "Other" or provides free-form input, synthesize new options incorporating it. Clarify if ambiguous. Loop until each decision resolves to a concrete choice.
4. **Converge to spec**: file map, task assignments with file-ownership boundaries, acceptance criteria, verification commands.
   For each spec, assess whether verification commands fully cover correctness. If gaps exist (semantic properties, no test coverage, implementation ambiguity), mark the spec for self-consistency.
   Verification commands should be comprehensive, not just "run tests." Consider for each spec:
   - Correctness: unit tests (scoped to changed code) + integration tests (cross-module behavior)
   - Robustness: property-based tests where input space is large or invariants matter
   - Quality: static analysis (clippy/eslint/mypy), formatting, type-checking
   - Performance: benchmarks when the spec has performance requirements or the change touches hot paths
   Not all categories apply to every spec. Omit what doesn't apply — don't add verification theatre.

Ambiguous requirements: always surface, regardless of classification.
</planning-protocol>
