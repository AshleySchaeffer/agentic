<planning-protocol>
1. **Classify**: does this extend an existing module, pattern, or convention? If yes → pattern-match. If it needs a new abstraction, data model, API shape, or architectural boundary → novel.
2. **Validation gates**  - `.claude/project-config.md` contents (if the file exists) are injected alongside this protocol. If the contents are present, use them. If flagged as missing and this task requires build/test/verification commands, spawn the config-gen agent to generate it before converging to spec. Then evaluate whether the current task introduces validation needs not yet in the config:
   - Tooling already listed in `.claude/project-config.md` is automatically included in the spec's verification commands  - do not ask.
   - If lint/format tooling is absent from the config, surface the gap: "This project has no [linter/formatter] configured  - should we add one?" via AskUserQuestion.
   - For new validation types the task may need (benchmarks, integration tests, property tests), surface as multi-select via AskUserQuestion.
   - Update `.claude/project-config.md` with confirmed additions and commit before converging to spec.
3. **Surface decisions via AskUserQuestion** - 2-3 options with pros/cons and a recommendation per decision point. Batch up to 4 questions per call.
   - **Pattern-match** (extends existing): scope and design decisions only. Make technical implementation calls autonomously - the codebase already answers "how."
   - **Novel** (new abstraction): both design and technical implementation decisions. The user decides how it is built, not just what. Require function signatures, data flow description, and edge case enumeration in the spec.
4. **Iterate**: when the user selects "Other" or provides free-form input, synthesize new options incorporating it. Clarify if ambiguous. Loop until each decision resolves to a concrete choice.
5. **Converge to spec**: file map, task assignments with file-ownership boundaries, acceptance criteria, verification commands.

   **Scope section**  - every spec must include a `## Scope` heading with one file path per line prefixed with `- `. This is the contract: the dev_stop hook parses it from the agent's transcript to enforce scope mechanically. No scope section = no mechanical enforcement.

   **Shared file detection**  - before execution, list all files per agent. Any file appearing in multiple agents' scopes must be resolved: serialize those tasks, or assign the file to one agent with the other's changes as a follow-up. Conflicts at merge time = planning error.

   Verification commands derive from `.claude/project-config.md` gates plus any task-specific additions from step 2.
   All non-simple specs use self-consistency by default.
   Consider for each spec:
   - Correctness: unit tests (scoped to changed code) + integration tests (cross-module behavior)
   - Robustness: property-based tests where input space is large or invariants matter
   - Quality: static analysis (clippy/eslint/mypy), formatting, type-checking
   - Performance: benchmarks when the spec has performance requirements or the change touches hot paths
   Not all categories apply to every spec. Omit what doesn't apply - don't add verification theatre.

Ambiguous requirements: always surface, regardless of classification.
</planning-protocol>
