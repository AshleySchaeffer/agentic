Update the current project's agentic workflow agent definitions with the latest versions.

Uses the agent definitions already installed globally at `~/.claude/agents/` by `/agentic-setup`.
Run `/agentic-setup` first to pull the latest definitions from GitHub.

`CLAUDE.md` is never overwritten — only agent definition files are updated.

## Steps

1. **Update agent definitions**
   - Create `.claude/agents/` in the current directory if it does not exist
   - Copy from `~/.claude/agents/` to `.claude/agents/`, overwriting any existing files:
     `architect.md`, `challenger.md`, `code-analyst.md`, `code-quality-auditor.md`,
     `data-analyst.md`, `dev.md`, `documentation-writer.md`, `qa.md`

2. **Report** each file that was updated and whether it was newly created or replaced
