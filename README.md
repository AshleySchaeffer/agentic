# agentic

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

Inside any project directory, run `/agentic-init`. This copies the agent definitions from
`~/.claude/agents/` into the project's `.claude/agents/` directory and creates a `CLAUDE.md`
template.

## Keeping up to date

Run `/agentic-setup` to pull the latest agent definitions from GitHub and update `~/.claude/agents/`.
Then run `/agentic-update` in any project to sync its local `.claude/agents/` from the updated globals.

## Contributing

To contribute improvements to the agent definitions, run `/agentic-contribute` from inside the
agentic repo. It opens a guided workflow to commit your changes and raise a pull request.
