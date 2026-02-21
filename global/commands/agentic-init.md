Initialize the current project with the agentic workflow setup.

Uses the agent definitions already installed globally at `~/.claude/agents/` by `setup.sh`.

## Steps

1. **Create project CLAUDE.md** (skip if `CLAUDE.md` already exists in cwd)
   - Infer project name from the current directory name
   - Write the following template:
     ```
     # CLAUDE.md

     ## Project

     **<dirname>** — [description]

     @project-config.md
     ```

2. **Install agent definitions**
   - Create `.claude/agents/` in the current directory if it does not exist
   - Copy from `~/.claude/agents/` to `.claude/agents/`, skipping any file that already exists:
     `architect.md`, `challenger.md`, `code-analyst.md`, `code-quality-auditor.md`,
     `data-analyst.md`, `dev.md`, `documentation-writer.md`, `qa.md`

3. **Report** each file that was created and each that was skipped (already existed)
