# Agentic

Agentic workflow system for [Claude Code](https://docs.anthropic.com/en/docs/claude-code). Provides
a structured multi-agent orchestration framework — architect, dev, QA, analyst, challenger, and
documentation agents — along with global configuration and slash commands.

## Prerequisites

- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) installed and authenticated
- [`gh` CLI](https://cli.github.com/) installed and authenticated

## Global setup

Run once on any new machine:

```bash
git clone https://github.com/AshleySchaeffer/agentic.git
cd agentic
./claude-code/setup.sh
```

This installs the `/agentic-*` slash commands into `~/.claude/commands/`. Then, inside Claude Code,
run `/agentic-setup` to complete setup:

- Installs agent definitions into `~/.claude/agents/`
- Installs orchestration rules into `~/.claude/`
- Installs global `CLAUDE.md` (skipped if one already exists)
- Enables Claude team mode in `~/.claude/settings.json`

Restart Claude Code after running `/agentic-setup`.

## Per-project setup

Inside any project directory, run `/agentic-init`. It fetches the branch list from your `agentic`
GitHub repo and, if more than one branch exists, presents an interactive picker. The selected branch
is pinned to `.claude/agentic-branch`. Agent definitions are then installed into `.claude/agents/`
and a `CLAUDE.md` template is created if one does not already exist.

Branch behaviour:

- **Default branch** — updates `~/.claude/agents/` globally, then copies from there into the project
- **Any other branch** — clones that branch directly into `.claude/agents/`; globals are not touched

Re-run `/agentic-init` at any time to switch branches. The picker highlights the currently pinned
branch so you can see what is active and change it if needed.

## Keeping up to date

Run `/agentic-update` in any project to pull the latest agent definitions. It reads the pinned branch
from `.claude/agentic-branch` (defaulting to the repo's default branch if no pin exists) and updates
`.claude/agents/` accordingly. On the default branch it also refreshes `~/.claude/agents/` first.

To update the global slash commands themselves, re-run `./claude-code/setup.sh` from the agentic repo
and then run `/agentic-setup` to refresh the global agent definitions and orchestration rules.

## Contributing

To contribute improvements to the agent definitions, run `/agentic-contribute` from inside the
agentic repo. It opens a guided workflow to commit your changes and raise a pull request.
