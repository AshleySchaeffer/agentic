<planning-protocol>
1. **Classify**: does this extend an existing module, pattern, or convention? If yes → pattern-match. If it needs a new abstraction, data model, API shape, or architectural boundary → novel.
2. **Surface decisions via AskUserQuestion** — 2-3 options with pros/cons and a recommendation per decision point. Batch up to 4 questions per call.
   - **Pattern-match**: scope and design decisions only. Make technical implementation calls autonomously — the codebase already answers "how."
   - **Novel**: both design AND technical implementation decisions. The user decides how it is built, not just what.
3. **Iterate**: when the user provides custom input, synthesize new options incorporating it. Clarify if ambiguous. Loop until each decision resolves to a concrete choice.
4. **Converge to spec**: file map, task assignments with file-ownership boundaries, acceptance criteria, verification commands.
   For each spec, assess whether verification commands fully cover correctness. If gaps exist (semantic properties, no test coverage, ambiguous implementation), mark the spec for self-consistency.

Ambiguous requirements: always surface, regardless of classification.
</planning-protocol>
