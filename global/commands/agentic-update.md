Pull the latest agent definitions from GitHub and update the current project.

$ARGUMENTS may contain a local filesystem path to the agentic repo. If empty, clones
from the current user's `agentic` GitHub repo.

`CLAUDE.md` is never overwritten — only agent definition files are updated.

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

3. **Update project agent definitions**
   - Create `.claude/agents/` in the current directory if it does not exist
   - Copy from `~/.claude/agents/` to `.claude/agents/`, overwriting any existing files:
     `architect.md`, `challenger.md`, `code-analyst.md`, `code-quality-auditor.md`,
     `data-analyst.md`, `dev.md`, `documentation-writer.md`, `qa.md`

4. **Clean up** the temp clone directory if one was created

5. **Report** each file that was updated and whether it was newly created or replaced
