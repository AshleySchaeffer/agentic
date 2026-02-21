# agentic

Agentic workflow system for [Claude Code](https://docs.anthropic.com/en/docs/claude-code). Provides
a structured multi-agent orchestration framework — architect, dev, QA, analyst, challenger, and
documentation agents — along with global configuration and slash commands.

## Prerequisites

- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) installed and authenticated
- [`gh` CLI](https://cli.github.com/) installed and authenticated (required for `/contribute`)

## Global setup

Run once on any new machine to install the workflow into `~/.claude/`:

```bash
git clone https://github.com/AshleySchaeffer/agentic.git
cd agentic
./claude-code/setup.sh
```

`setup.sh` installs:

| Source | Destination | Behaviour |
|---|---|---|
| `global/CLAUDE.md` | `~/.claude/CLAUDE.md` | Skip if already exists |
| `claude-code/orchestration-rules.md` | `~/.claude/orchestration-rules.md` | Always overwrite |
| `claude-code/*.md` | `~/.claude/agents/` | Always overwrite |
| `global/commands/*.md` | `~/.claude/commands/` | Always overwrite |

Restart Claude Code after running `setup.sh`.

## Per-project setup

Inside any project directory, run the `/agentic-init` slash command in Claude Code. This copies
the agent definitions from `claude-code/` into the project's `.claude/agents/` directory and
creates a `project-config.md` documenting components, build systems, and verification commands.

## Keeping up to date

```bash
cd agentic
git pull
./claude-code/setup.sh
```

Or, from inside any project that has been initialised with `/agentic-init`, run the `/update`
slash command. It pulls the latest agent definitions from this repo and copies them into the
project's `.claude/agents/` directory.

## Contributing

To contribute improvements to the agent definitions, run the `/contribute` slash command from
inside any initialised project. It opens a guided workflow to commit your changes and raise a
pull request against this repo.
