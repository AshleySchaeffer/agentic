# Adversarial Review of DESIGN.md (Round 2)

## What the previous review got right but undersold

**Point #4 (orchestrator solves symptoms) is the strongest criticism and it was buried at position 4.** The contradiction is fatal: the document spends 3 pages arguing that coordination complexity is the problem, then proposes a Rust daemon with SQLite, a TUI, event sourcing, hook intercepts, and context injection as the solution. If three agents with minimal coordination is genuinely the answer, the orchestrator is over-engineered. If the orchestrator is genuinely needed, three agents with minimal coordination isn't the answer. The document never resolves this because it wants both claims simultaneously.

**Point #9 (silent reintroduction of specialization) deserved more teeth.** The document doesn't just reintroduce specialization — it reintroduces it *without the contract*. The current system's QA agent has a defined interface: it receives requirements, produces tests, reports coverage. The new "implementer instance scoped to testing" has... nothing. No defined input contract, no output expectations, no verification criteria distinct from any other implementer. The document deleted the specification while keeping the work.

---

## What the previous review missed entirely

### 1. The reviewer role is underspecified to the point of being decorative

The reviewer has two touchpoints: pre-user decision clusters and pre-execution plan review. At the first touchpoint, it's checking the planner's *framing* of options. But what does "catching anchoring bias" actually mean operationally? The reviewer receives the planner's output and... does what? It has no independent investigation capability described. It doesn't read the code separately. It doesn't run its own analysis. It reviews the planner's summary of the code.

This is peer review of a book report without reading the book. The reviewer can catch logical inconsistencies in the planner's presentation, but it cannot catch *omissions* — things the planner didn't mention because the planner didn't look. The most dangerous form of bias isn't misframing what you found; it's not looking where the answer might disagree with you. The reviewer as described has no mechanism to catch this.

### 2. The document confuses "protocol overhead" with "domain instruction"

The central claim is that current agent prompts are bloated with protocol and the new system eliminates this. But look at what a planner prompt actually needs:

- How to structure decision clusters
- When a cluster is "coherent" enough to present
- How to present options without anchoring
- When to spawn exploration subtasks vs. read directly
- How to incorporate user feedback and continue
- How to assemble resolved clusters into an execution plan
- How to scope implementer tasks from the plan

This is *more* prompt content than the current architect needs for its coordination duties, not less. The difference is that it's dressed up as "domain instruction" rather than "protocol." The planner's prompt is a coordination protocol — it's just coordinating with the user instead of with other agents. Calling it something different doesn't make the prompt shorter.

### 3. "Decision clusters" are a UX concept without an operational definition

The document says the planner groups decisions into "coherent clusters at natural decision boundaries." Three to five clusters is "typical." But:

- What makes a boundary "natural"? The planner has to decide, which means the planner's judgment about what decisions are related *shapes what the user sees*. This is exactly the framing bias the reviewer is supposed to catch, but the cluster boundaries are set before the reviewer sees them.
- If the planner gets the clustering wrong — groups two independent decisions together, or splits a tightly coupled pair — the user makes choices based on a false model of decision interdependence. There's no recovery mechanism described.
- "Three to five" is a suspiciously round number that sounds like UX intuition, not analysis. A database migration touching four tables, an API redesign, and a frontend rewrite might have 12 genuine decision points. Does the planner compress these into 4 clusters, losing granularity? Or present 12, violating the design's own principle?

### 4. The document has no theory of failure for the new system

Every critique of the current system is accompanied by a failure mode: "agents drop protocol steps," "teams hang silently," "the user reviews a fait accompli." These are specific, observable, debuggable.

The new system's failure modes are never discussed:

- What happens when the planner's context fills mid-investigation and it loses track of earlier findings? (The current system externalizes to disk; the planner apparently keeps everything in-context.)
- What happens when the user makes a choice in cluster 2 that invalidates the planner's assumptions in cluster 4, but the planner doesn't realize this until cluster 4?
- What happens when two parallel implementers produce code that individually passes verification but is incompatible when integrated?
- What happens when the reviewer misses something? There's no second reviewer, no escalation, no fallback. One miss and the error propagates to execution.

A design that doesn't describe its own failure modes hasn't been stress-tested. It's optimized for the happy path.

### 5. The exploration subtask escape hatch is load-bearing but treated as minor

The planner "spawns lightweight exploration subtasks" when the codebase is too large. This one subordinate clause is doing enormous structural work. For any real codebase — not a toy project, but something with 500+ files across multiple packages — the planner *will* need parallel investigation. Which means:

- The planner needs to decide what to investigate in parallel (task decomposition)
- The subtasks need to report findings in a useful format (interface contract)
- The planner needs to synthesize multiple parallel findings (coordination)
- The subtasks might find things that change what other subtasks should look for (dynamic re-scoping)

This is the analyst pattern with extra steps. The document treats it as an optimization the planner can optionally use, but for non-trivial work it's the *primary mode of operation*. The design buries its most important interaction pattern in a parenthetical.

### 6. The implementer's "self-contained" claim doesn't survive contact with reality

"Self-contained — it gets everything it needs to execute from the plan, does its work, runs verification, and delivers."

This assumes the plan can specify implementation at a level of detail sufficient for an agent to execute without questions. But plans operate at the level of "implement auth middleware that validates JWT tokens and attaches user context." The implementer will encounter:

- Ambiguous type signatures the plan didn't specify
- Edge cases the plan didn't enumerate
- Existing code patterns that conflict with the plan's approach
- Test infrastructure that doesn't support the verification the plan assumed

In the current system, these bubble up through the architect. In the new system, the implementer... does what? Goes back to the planner? The planner's context is full of planning state, not implementation details. Asks the user? The user was told the plan was complete. Makes its own judgment call? Then it's making design decisions the planner was supposed to own.

"Self-contained" works when the spec is complete. Specs are never complete. The document doesn't address the inevitable flow of implementation-time questions back to the planning layer.

### 7. The document never justifies *three* roles specifically

Why three? The argument against eight roles is that coordination overhead exceeds the benefit. But the document never establishes that three is the right number rather than two (planner + implementer, with review as a planner sub-mode) or four (planner + reviewer + implementer + integrator). The number three appears to be aesthetic — it feels simpler than eight — not derived from any principle.

If the argument is "minimize roles to reduce coordination," one role is the minimum. A single agent that plans, reviews its own plan with a fresh prompt, and implements would have zero coordination overhead. The document rejects this implicitly but never explains why the reviewer *must* be a separate agent while the analyst *must not* be.

---

## Where the previous review was wrong

**Point #7 (user-in-the-loop doesn't scale) overcorrects.** The previous review says users "prefer reviewing a complete plan to being drip-fed individual decisions." This is true for routine decisions but false for architectural ones. The current system's failure mode — user discovers at review time that eight autonomous steps produced a design they hate — is real and expensive. The right criticism isn't that user involvement is bad; it's that the document doesn't distinguish between decisions that need user input and decisions the planner should make autonomously. A good design would classify decision types, not treat all decisions uniformly.

**Point #5 (hook latency) is technically correct but strategically wrong.** DESIGN.md explicitly says the orchestrator hooks only orchestration-relevant tools (SendMessage, TaskCreate, etc.), *not* Read/Grep/Glob/Bash. The previous review's latency critique applies to a system the document doesn't propose. The real concern with hooks isn't latency — it's reliability. A hook that fails to fire means the orchestrator's state diverges from reality. Silent divergence in the coordination layer is worse than the silent protocol drops the document criticizes.

---

## Summary

DESIGN.md is a well-written argument for a position it arrived at before the analysis. The diagnosis of the current system's problems is sharp and mostly correct. The proposed solution doesn't follow from the diagnosis — it follows from an aesthetic preference for fewer moving parts, then reverse-engineers justifications. The orchestrator exists because three agents can't actually coordinate themselves, which undermines the premise that fewer agents means less coordination. The planner concentrates more responsibility than the architect it replaces. The reviewer lacks the tools to do its stated job. The implementer's "self-contained" promise requires plan completeness that planning can't deliver. And the exploration subtask escape hatch, buried in a subordinate clause, is where the actual system design lives — unnamed, unspecified, and structurally identical to what was eliminated.
