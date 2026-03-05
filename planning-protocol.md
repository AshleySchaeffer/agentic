<planning-protocol>
1. **Classify**: does this extend an existing module, pattern, or convention? If yes → pattern-match. If it needs a new abstraction, data model, API shape, or architectural boundary → novel.
2. **Validation gates** — read `project-config.md` (via `@project-config.md`). If it doesn't exist and session_start didn't already prompt for bootstrap, spawn the config-gen agent to generate it now. Then evaluate whether the current task introduces validation needs not yet in the config:
   - Tooling already listed in project-config.md is automatically included in the spec's verification commands — do not ask.
   - If lint/format tooling is absent from the config, surface the gap: "This project has no [linter/formatter] configured — should we add one?" via AskUserQuestion.
   - For new validation types the task may need (benchmarks, integration tests, property tests), surface as multi-select via AskUserQuestion.
   - Update project-config.md with confirmed additions and commit before converging to spec.
3. **Surface decisions via AskUserQuestion**  - 2-3 options with pros/cons and a recommendation per decision point. Batch up to 4 questions per call.
   - **Pattern-match** (extends existing): scope and design decisions only. Make technical implementation calls autonomously  - the codebase already answers "how."
   - **Novel** (new abstraction): both design and technical implementation decisions. The user decides how it is built, not just what.
4. **Iterate**: when the user selects "Other" or provides free-form input, synthesize new options incorporating it. Clarify if ambiguous. Loop until each decision resolves to a concrete choice.
5. **Converge to spec**: file map, task assignments with file-ownership boundaries, acceptance criteria, verification commands.
   Verification commands derive from project-config.md gates plus any task-specific additions from step 2.
   All non-simple specs use self-consistency by default. Verification commands should still be comprehensive  - they are the primary quality gate for comparing SC results.
   Verification commands should be comprehensive, not just "run tests." Consider for each spec:
   - Correctness: unit tests (scoped to changed code) + integration tests (cross-module behavior)
   - Robustness: property-based tests where input space is large or invariants matter
   - Quality: static analysis (clippy/eslint/mypy), formatting, type-checking
   - Performance: benchmarks when the spec has performance requirements or the change touches hot paths
   Not all categories apply to every spec. Omit what doesn't apply  - don't add verification theatre.

Ambiguous requirements: always surface, regardless of classification.
</planning-protocol>
