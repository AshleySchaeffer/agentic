# Agentic

A Claude Code multi-agent configuration. Two agents, five hooks, one CLAUDE.md under 120 lines.

## Why

Multi-agent coding setups fail in predictable ways. Agentic addresses the three that matter most.

**Agent teams don't scale.** Communication overhead grows super-linearly beyond 3-4 agents ([Kim et al. 2025](https://arxiv.org/abs/2512.08296)), and 2 diverse agents match or exceed 16 homogeneous ones ([Li et al. 2025](https://arxiv.org/abs/2602.03794)). As base models improve, the multi-agent advantage shrinks from ~10% to ~3% ([Chen et al. 2025](https://arxiv.org/abs/2505.18286)). Agentic uses 2 task-oriented agents (dev on Sonnet, reviewer on Opus) with the main Opus session as architect — model diversity over headcount.

**Instructions get ignored.** Claude follows each instruction with ~90% accuracy, but 10 simultaneous instructions compound to ~35% ([Curse of Instructions, ICLR 2025](https://openreview.net/forum?id=R6q67CDBCH)). Claude Code's system prompt already consumes ~50 instructions before your CLAUDE.md loads ([IFScale, Distyl AI 2025](https://arxiv.org/abs/2507.11538)). Structured prompts survive compaction with 92% fidelity vs 71% for prose. Agentic keeps CLAUDE.md under 120 lines with XML directive tags, progressive disclosure to external files, and hook-injected protocols that cost zero context until triggered.

**LLM-on-LLM review doesn't work.** Multi-Agent Debate frameworks fail to consistently beat Self-Consistency — sampling the same model twice and voting ([ICLR 2025 MAD evaluation](https://iclr.cc/virtual/2025/poster/31346), [Smit et al. ICML 2024](https://arxiv.org/abs/2311.17371)). When agents share training data, debate converges to shared wrong answers ([Estornell & Liu, NeurIPS 2024](https://openreview.net/forum?id=sy7eSEXdPC)). Agentic uses automated verification (tests, types, linting) as the primary quality gate, with a focused reviewer agent only for high-stakes semantic concerns.

## Setup

```bash
cargo build --release
./target/release/agentic install
```

Installs to `~/.claude/`:

| Artifact | Path | Purpose |
|---|---|---|
| CLAUDE.md | `~/.claude/CLAUDE.md` | Architect protocol — task routing, planning, coordination, coding standards |
| coding-standards.md | `~/.claude/coding-standards.md` | Full standards (progressive disclosure from CLAUDE.md) |
| dev agent | `~/.claude/agents/dev.md` | Sonnet — implements against complete specs |
| reviewer agent | `~/.claude/agents/reviewer.md` | Opus — focused review with task-specific checklist |
| hooks binary | `~/.local/bin/agentic` | 5 hooks compiled to a single binary |
| settings.json | `~/.claude/settings.json` | Hook matchers + agent teams flag |

To refresh project config: `agentic refresh`
To uninstall: `agentic uninstall`

## Architecture

The main Claude Code session (Opus) acts as the architect directly — no routing layer. It investigates, plans, coordinates dev agents, and runs the quality gate. This follows the [orchestrator-worker pattern](https://www.anthropic.com/research/building-effective-agents) but eliminates the overhead of a separate routing instance.

**Task routing** scales with complexity:

| Complexity | Approach |
|---|---|
| Simple fixes | Handle directly, no agents |
| Everything else | Self-consistency — 2 devs per task in separate worktrees with same spec; parallel work splits into sub-tasks with file-ownership, each getting its own SC pair |

**Planning** adapts to novelty. On plan mode entry, a hook injects a classification protocol:
- **Pattern-match** (extending existing code): scope and design decisions only — technical calls made autonomously
- **Novel** (new abstractions, APIs, boundaries): both design AND implementation decisions surfaced to the user

**Dev coordination** is spec-driven. Each agent receives files to change, acceptance criteria, and verification commands. No sign-off round-trips — the spec is the contract ([communication overhead is super-linear](https://arxiv.org/abs/2512.08296)). File ownership prevents merge conflicts: every file has exactly one owner.

**Quality gate** before completion: all tasks done, full build + test suite passes, reviewer findings resolved (if spawned).

## Agents

| Agent | Model | Role |
|---|---|---|
| dev | Sonnet | Implements against complete specs autonomously |
| reviewer | Opus | Focused review with task-specific checklist (high-stakes only) |

Agents are task-oriented, not role-bound. A dev receives a scoped task (files, criteria, verification commands) and works autonomously. The reviewer is spawned when a change introduces risks that verification commands don't address — security-sensitive code, schema migrations, architectural shifts, concurrency, public API surface — with a task-specific checklist of lenses, not open-ended critique.

## Hooks

| # | Trigger | What it does |
|---|---|---|
| 1 | PreToolUse/SendMessage | Offloads large messages (>4KB, >2KB with code) to disk, replaces with file reference |
| 2 | PreToolUse/Agent | Forces `acceptEdits` mode on all agent spawns |
| 3 | PostToolUse/Bash | Re-injects project-config.md content after git commit/merge/pull, verifies @reference |
| 4 | PreToolUse/EnterPlanMode | Injects adaptive planning protocol (pattern-match vs novel classification) |
| 5 | SessionStart | Ensures `@project-config.md` in project CLAUDE.md, bootstraps project-config.md if missing |

Hooks enforce what prompts cannot guarantee ([real-world verification outperforms LLM-on-LLM review](https://arxiv.org/abs/2311.17371)). Hook 4 is zero-cost — the planning protocol lives in the binary and is injected only when plan mode is entered, keeping CLAUDE.md lean.

## Design Decisions

1. **Main session is the architect** — Opus wasted on routing contradicts the finding that model choice explains most performance variance ([Anthropic 2024](https://www.anthropic.com/research/building-effective-agents))
2. **Fewer task-oriented agents** — communication overhead is super-linear beyond 3-4 agents; diversity beats headcount ([Kim et al. 2025](https://arxiv.org/abs/2512.08296), [Li et al. 2025](https://arxiv.org/abs/2602.03794))
3. **Self-consistency as default** — MAD frameworks don't reliably beat sampling twice and voting ([ICLR 2025](https://iclr.cc/virtual/2025/poster/31346), [Smit et al. 2024](https://arxiv.org/abs/2311.17371)). SC applies to all non-simple tasks, with automated verification as the primary quality gate. Blocking reviewer findings are fixed through SC (one round) — same confidence guarantee on fixes as on original implementations. Only simple fixes skip SC — they don't justify the overhead
4. **Upfront specs, no sign-off cycles** — each round-trip compounds super-linear overhead ([Kim et al. 2025](https://arxiv.org/abs/2512.08296))
5. **Context-isolated TDD** — test-driven development improves code generation by 12.78% on MBPP; context isolation requires task scoping, not a separate agent
6. **XML directive tags** — structured prompts survive compaction with 92% fidelity vs 71% for prose
7. **CLAUDE.md under 120 lines** — instruction compliance degrades exponentially with count ([Curse of Instructions, ICLR 2025](https://openreview.net/forum?id=R6q67CDBCH)); Claude Code's system prompt already consumes ~1/3 of the instruction budget ([IFScale](https://arxiv.org/abs/2507.11538))
8. **Complexity-based routing** — multi-agent advantage shrinks to ~3% as models improve ([Chen et al. 2025](https://arxiv.org/abs/2505.18286)); simple tasks don't justify the overhead
9. **Model diversity** — Opus for orchestration/review, Sonnet for implementation, Haiku for exploration ([Li et al. 2025](https://arxiv.org/abs/2602.03794))
10. **Hook-injected planning protocol** — zero context cost until plan mode entry; hooks enforce what prompts cannot

## Files

```
architect.md               # Architect protocol → ~/.claude/CLAUDE.md
coding-standards.md        # Full standards (progressive disclosure)
planning-protocol.md       # Planning protocol (injected via hook 4)
Cargo.toml                 # Binary: agentic
src/main.rs                # 5 hooks + install/uninstall/refresh
agents/
  dev.md                   # Implementation agent (Sonnet)
  reviewer.md              # Review agent (Opus, high-stakes only)
```
