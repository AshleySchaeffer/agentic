# Adversarial Review of DESIGN.md and IDEA.md

## 1. "Role specialization doesn't help LLMs" is asserted, not proven

DESIGN.md claims: "An LLM doesn't write better tests because you called it a QA specialist. It writes better tests because you gave it clear requirements and focused context."

This conflates two things. Role specialization *is* a mechanism for providing focused context. A QA agent prompt that says "you are a test engineer, here are the testing patterns in this codebase, here are the coverage gaps" is doing exactly what the document advocates — giving clear requirements and focused context. The role name is just shorthand for a prompt shape.

The real question isn't whether roles help, it's whether the *coordination cost* of separate roles exceeds the *context focus benefit*. The document assumes the answer is always yes, but never measures it. For a 200-file implementation touching auth, database, and frontend, a single "implementer" instance will have a worse signal-to-noise ratio in its context window than three scoped agents would. The document hand-waves this with "multiple implementers run in parallel" but these are still generalist prompts — you've just recreated role specialization without the name.

## 2. The Planner bottleneck is worse than the Architect bottleneck

The document criticizes the current architect for spending "most of its context tracking who said what, dispatching unblocks, relaying messages." The proposed planner does *more*: it reads all the code itself, reasons about the problem, presents options to the user, and iterates. The architect at least delegated investigation.

The planner is a single serial reasoning process handling the entire problem space. For any non-trivial codebase, the planner's context window fills with code from Area A while it needs to reason about Area B. The document acknowledges this — "when the codebase is large enough that parallel investigation would help, it spawns lightweight exploration subtasks" — but this is exactly the analyst pattern it just eliminated, minus the clear contract about what to report back. You've replaced a named role with an unnamed one and lost the interface definition.

## 3. "One round of review, no debate loops" is a fragility, not a feature

DESIGN.md presents single-round review as a design choice: "One agent, two touchpoints. One round of review at each, no debate loops."

This assumes the reviewer catches everything on the first pass. But the reviewer is reviewing the planner's *framing* of options — if the planner's bias is structural (the framing itself obscures the issue), a single review pass won't catch it because the reviewer is operating within the planner's frame. Debate loops exist because the first challenge often surfaces information that changes what the *second* challenge should look at. Capping at one round trades correctness for predictability.

## 4. The orchestrator solves problems that are symptoms, not causes

IDEA.md lists four problems: message delivery, task tracking, large message duplication, and context compaction. Three of these are symptoms of having too many agents communicating too much.

- **Message delivery failures** scale with message volume. Fewer agents = fewer messages = fewer failures.
- **Task dependency tracking** complexity scales with task count and interdependency. Simpler task graphs need less tracking.
- **Large message context duplication** is a consequence of agents needing to share findings they gathered separately. If the planner does its own investigation (as DESIGN.md proposes), this problem largely disappears.

The orchestrator is a sophisticated solution to problems that the new architecture claims to eliminate. If three roles with minimal coordination is the answer, you shouldn't need a Rust daemon with SQLite, a TUI, automation engines, and 12 hook intercepts to keep them coordinated. The complexity of the orchestrator contradicts the simplicity thesis of the design.

## 5. Hook latency is hand-waved

Every tool call in the system — every `Read`, every `Grep`, every `Bash` — goes through a hook handler that opens a unix socket, sends data to a daemon, waits for a response, and returns. IDEA.md acknowledges this in the open questions: "Profile the unix socket round-trip to the daemon. If latency is problematic for high-frequency tools..."

This isn't an open question — it's a known problem. A planner doing its own investigation will call Read/Grep/Glob hundreds of times. Adding even 10ms of latency per call means seconds of overhead per investigation cycle. The suggestion to "skip interception for tools where the orchestrator only needs activity timestamps" undermines stall detection accuracy and alert injection, which are the primary value propositions of those intercepts.

## 6. additionalContext injection is a context window tax

The orchestrator injects alerts, unblock notifications, and state reminders into agents' tool calls via `additionalContext`. Every injection consumes context window space. The document says "only inject when there's an actionable item" but in a busy team, actionable items accumulate:

- 2 stalled tasks to be aware of
- 1 dead agent alert
- 3 newly unblocked tasks
- Recovery context after compaction

This is exactly the protocol overhead the design claims to eliminate from agent prompts — it's just moved from static prompt text to dynamic injection. The agent still has to parse and act on coordination information instead of doing its job.

## 7. The "user in the loop" planning model doesn't scale

DESIGN.md's planning flow requires the user to make every decision point: "read code, identify a decision point, present options to the user, incorporate the user's choice, continue."

For a task with 15 decision points, this means 15 interruptions. The current system's autonomous pipeline exists *because* users don't want to be interrupted 15 times. They want to review a coherent proposal and approve or reject it. The document frames the current approach as presenting a "fait accompli" — but most users prefer reviewing a complete plan to being drip-fed individual decisions without seeing how they compose.

The document doesn't address decision interdependence: choice A at decision point 3 may invalidate the options at decision point 7. The planner has to either backtrack (wasting the user's previous decisions) or present decisions in dependency order (which requires the pipeline-style analysis the document rejects).

## 8. Compaction recovery is not as clean as claimed

IDEA.md reduces compaction handling to: "If you've lost context, call `orchestrator:recover`." But the agent has to *know* it's lost context to call recover. The document identifies this in open questions: "Can the orchestrator detect when an agent has been compacted?"

If the agent doesn't know it's been compacted, it continues operating on stale context — making decisions based on a conversation history that's been summarized and potentially distorted. The orchestrator can detect activity gaps, but by the time it injects recovery context on the next tool call, the agent may have already made decisions based on the compacted state. There's a window of vulnerability that the event store can't close.

## 9. The three-role model silently reintroduces specialization

Look at what the Implementer actually does: "writes code, tests, and documentation against a clear spec." This is three activities with different success criteria, different verification methods, and different failure modes. Combining them means:

- Tests written by the same agent that wrote the code share the same blind spots
- Documentation written by the implementer will reflect the implementer's understanding, not the user's mental model
- A single agent context holds production code, test code, and documentation — reducing the space available for each

The current system separated these for a reason. The document argues the reason was "mirroring a human org chart," but there's a stronger reason: adversarial independence. Tests written by a different agent than the code are more likely to catch bugs because they represent a different interpretation of the spec.

## 10. The design assumes its conclusion

The core structure of the argument is: "LLMs are bad at coordination protocols → move coordination to infrastructure → agent prompts become simple." But the orchestrator doesn't eliminate coordination — it hides it. Agents still need to understand task dependencies, respond to unblock notifications, handle dead teammate alerts, and recover from compaction. They just receive this information through injected context instead of prompt rules.

The difference between "your prompt says to check blockedBy before starting" and "the orchestrator injects 'Task X is now unblocked, proceed'" is mechanical, not conceptual. The agent still has to process coordination information and act on it correctly. The orchestrator makes delivery reliable, but it doesn't make agents better at *acting on* coordination signals — which is where the actual failures occur.

---

## Counter-thesis

The current system's problems are real, but the proposed solution replaces visible complexity (long prompts with explicit protocols) with hidden complexity (a Rust daemon intercepting every tool call). When hidden coordination fails, it fails silently and is harder to debug. The three-role simplification works for small tasks but reintroduces specialization through the back door for large ones. The user-in-the-loop planning model is correct for high-stakes decisions but exhausting for routine ones. The right answer is probably to fix the current architecture's specific failure modes (unblock dispatch, message delivery, stall detection) without rebuilding the entire agent model.
