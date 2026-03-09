# Agentic

A Claude Code multi-agent configuration. Hook-enforced invariants, minimal prompts, transcript-based scope enforcement.

## Why

Multi-agent coding setups fail in predictable ways. Agentic addresses the three that matter most.

**Agent teams don't scale.** Communication overhead grows super-linearly beyond 3-4 agents ([Kim et al. 2025](https://arxiv.org/abs/2512.08296)), and 2 diverse agents match or exceed 16 homogeneous ones ([Li et al. 2025](https://arxiv.org/abs/2602.03794)). Agentic uses 3 task-oriented agents (dev on Sonnet, reviewer on Opus, config-gen on Haiku) with the main Opus session as architect - model diversity over headcount.

**Instructions get ignored.** Claude follows each instruction with ~90% accuracy, but 10 simultaneous instructions compound to ~35% ([Curse of Instructions, ICLR 2025](https://openreview.net/forum?id=R6q67CDBCH)). Agentic applies one principle: if a hook can enforce it, the prompt is silent on it. Hooks guarantee invariants mechanically — dirty tree checks, worktree isolation, file scope enforcement. Prompts are reserved for judgment guidance only.

**LLM-on-LLM review doesn't work.** Multi-Agent Debate frameworks fail to consistently beat Self-Consistency - sampling the same model twice and voting ([ICLR 2025 MAD evaluation](https://iclr.cc/virtual/2025/poster/31346), [Smit et al. ICML 2024](https://arxiv.org/abs/2311.17371)). Agentic uses automated verification as the primary quality gate, with a focused reviewer agent only for high-stakes semantic concerns.

## Setup

```bash
cargo build --release
./target/release/agentic install
```

Installs to `~/.claude/`:

| Artifact | Path | Purpose |
|---|---|---|
| CLAUDE.md | `~/.claude/CLAUDE.md` | Architect protocol - task routing, planning, coordination |
| coding-standards.md | `~/.claude/coding-standards.md` | Full standards (progressive disclosure from CLAUDE.md) |
| dev agent | `~/.claude/agents/dev.md` | Sonnet - implements against complete specs |
| reviewer agent | `~/.claude/agents/reviewer.md` | Opus - focused review with task-specific checklist |
| config-gen agent | `~/.claude/agents/config-gen.md` | Haiku - scans project and generates project-config.md |
| hooks binary | `~/.cargo/bin/agentic` | 6 hook handlers compiled to a single binary |
| settings.json | `~/.claude/settings.json` | Hook matchers |

### Permissions

`agentic permissions` manages project-local permissions in `.claude/settings.local.json`. Four tiers, applied additively:

| Tier | Flag | What it allows |
|---|---|---|
| git | `--git` | `git status`, `diff`, `log`, `add`, `commit`, `merge`, `branch`, `worktree`, `stash`, `checkout`, `rev-parse` |
| readonly | `--readonly` | Read-only shell commands (`ls`, `cat`, `find`, etc.), `Read`, `Glob`, `Grep` |
| agent | `--agent` | `Agent` (spawn subagents) |
| write | `--write` | `Edit`, `Write`, `NotebookEdit` |

```bash
agentic permissions add              # Interactive — prompts for each tier
agentic permissions add --git --readonly --agent --write  # Non-interactive — all tiers
agentic permissions remove           # Remove all agentic-managed permissions
```

To uninstall: `agentic uninstall`

## Architecture

The main Claude Code session (Opus) acts as the architect - it investigates, plans, coordinates dev agents, and runs the quality gate. This follows the [orchestrator-worker pattern](https://www.anthropic.com/research/building-effective-agents).

### Hook-enforced invariants

Hooks enforce what prompts cannot guarantee. Every invariant that can be checked mechanically is a hook, not a prompt instruction.

| Hook | Trigger | Enforcement |
|---|---|---|
| `planning_protocol` | PreToolUse/EnterPlanMode | Injects planning protocol + project-config.md as context |
| `agent_spawn` | PreToolUse/Agent | Blocks on dirty working tree; injects `isolation: "worktree"` for dev agents |
| `bash_guard` | PreToolUse/Bash | Blocks cherry-pick/rebase in worktrees; auto-rebases stale branches before merge |
| `merge_cleanup` | PostToolUse/Bash | After `git merge`, removes the merged branch's worktree and deletes the branch |
| `dev_stop` | SubagentStop | Parses `## Scope` from agent transcript, blocks out-of-scope file changes; blocks on uncommitted changes or missing commits |
| `session_start` | SessionStart | Checks for git repo; detects nested projects |

### Transcript-based scope enforcement

The `dev_stop` hook reads the agent's conversation transcript to extract the `## Scope` section from the original spec. It compares `git diff --name-only` against the scope list and blocks if any files were modified outside scope. This replaces prompt-based scope lock instructions with mechanical enforcement.

Graceful degradation: if the transcript can't be read or has no `## Scope` section, the scope check is skipped.

### Task routing

| Complexity | Approach |
|---|---|
| Simple tasks | Single dev agent in a worktree |
| Everything else | Self-consistency - 2 devs per task with same spec |

### Planning

On plan mode entry, a hook injects a classification protocol:
- **Pattern-match** (extending existing code): scope and design decisions only
- **Novel** (new abstractions): design AND implementation decisions, with function signatures and data flow

Every spec includes a `## Scope` section with file paths — the contract for mechanical scope enforcement.

### Quality gate

Before completion: all tasks done, verification passes (via verifier agent), reviewer findings resolved (if spawned). The `dev_stop` hook ensures agents commit their work and stay within scope.

## Agents

| Agent | Model | Role |
|---|---|---|
| dev | Sonnet | Implements against complete specs autonomously |
| reviewer | Opus | Focused review with task-specific checklist (high-stakes only) |
| config-gen | Haiku | Scans project and generates/updates project-config.md |
| verifier | Haiku | Runs verification commands, returns pass/fail summary |

## Design Decisions

1. **Hook-enforced invariants** — if a hook can enforce it, the prompt is silent on it. Dirty tree checks, worktree isolation, and file scope are mechanical guarantees, not instructions to follow.
2. **Transcript-based scope enforcement** — the dev_stop hook parses the agent's transcript for the `## Scope` section, making scope violations impossible to commit rather than merely discouraged.
3. **Fewer task-oriented agents** — communication overhead is super-linear beyond 3-4 agents; diversity beats headcount ([Kim et al. 2025](https://arxiv.org/abs/2512.08296), [Li et al. 2025](https://arxiv.org/abs/2602.03794))
4. **Self-consistency as default** — MAD frameworks don't reliably beat sampling twice and voting ([ICLR 2025](https://iclr.cc/virtual/2025/poster/31346)). SC applies to all non-simple tasks.
5. **Upfront specs, no sign-off cycles** — each round-trip compounds super-linear overhead ([Kim et al. 2025](https://arxiv.org/abs/2512.08296))
6. **Minimal prompts** — instruction compliance degrades exponentially with count ([Curse of Instructions, ICLR 2025](https://openreview.net/forum?id=R6q67CDBCH)). Architect protocol is ~70 lines, dev prompt is ~25 lines.
7. **Model diversity** — Opus for orchestration/review, Sonnet for implementation, Haiku for config/verification ([Li et al. 2025](https://arxiv.org/abs/2602.03794))

## Files

```
architect.md               # Architect protocol → ~/.claude/CLAUDE.md (~70 lines)
coding-standards.md        # Full standards (progressive disclosure)
planning-protocol.md       # Planning protocol (injected via hook)
Cargo.toml                 # Binary: agentic
src/main.rs                # 6 hook handlers + install/uninstall/permissions
agents/
  dev.md                   # Implementation agent (Sonnet, ~25 lines)
  reviewer.md              # Review agent (Opus, high-stakes only)
  config-gen.md            # Config generation agent (Haiku)
  verifier.md              # Verification agent (Haiku)
```
