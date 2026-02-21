Initialize the current project with the agentic workflow setup.

Pulls the latest agent definitions from GitHub (same as `/agentic-update`), then creates
a project `CLAUDE.md` template if one does not already exist.

$ARGUMENTS may contain a local filesystem path to the agentic repo. If empty, clones
from the current user's `agentic` GitHub repo.

## Steps

1. **Determine source**
   - If `$ARGUMENTS` is empty:
     - Run `GH_USER=$(gh api user --jq '.login')` to get the GitHub username
     - Run `TMPDIR=$(mktemp -d) && gh repo clone ${GH_USER}/agentic ${TMPDIR}/agentic`
     - Set `SOURCE="${TMPDIR}/agentic"`
   - If `$ARGUMENTS` is a path: set `SOURCE="$ARGUMENTS"`
   - Verify `${SOURCE}/claude-code/` exists; abort with a clear message if not

2. **Update global agent definitions**
   - Create `~/.claude/agents/` if it does not exist
   - Copy from `${SOURCE}/claude-code/` to `~/.claude/agents/`, overwriting any existing files:
     `architect.md`, `challenger.md`, `code-analyst.md`, `code-quality-auditor.md`,
     `data-analyst.md`, `dev.md`, `documentation-writer.md`, `qa.md`

3. **Create project CLAUDE.md** (skip if `CLAUDE.md` already exists in cwd)
   - Infer project name from the current directory name
   - Write the following template:
     ```
     # CLAUDE.md

     ## Project

     **<dirname>** — [description]

     @project-config.md
     ```

4. **Install project agent definitions**
   - Create `.claude/agents/` in the current directory if it does not exist
   - Copy from `~/.claude/agents/` to `.claude/agents/`, skipping any file that already exists:
     `architect.md`, `challenger.md`, `code-analyst.md`, `code-quality-auditor.md`,
     `data-analyst.md`, `dev.md`, `documentation-writer.md`, `qa.md`

5. **Clean up** the temp clone directory if one was created

6. **Report** each file that was created and each that was skipped (already existed)
