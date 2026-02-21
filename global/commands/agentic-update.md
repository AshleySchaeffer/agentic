Update the current project's agentic workflow agent definitions with the latest versions.

$ARGUMENTS may contain a local filesystem path to a git repo containing a `claude-code/`
subdirectory. If empty, fetch the latest files from the current user's `agentic` GitHub repo.

`CLAUDE.md` is never overwritten — only agent definition files are updated.

## Steps

1. **Determine source**
   - If `$ARGUMENTS` is empty:
     - Run `GH_USER=$(gh api user --jq '.login')` to get the GitHub username
     - Run `TMPDIR=$(mktemp -d) && gh repo clone ${GH_USER}/agentic ${TMPDIR}/agentic` to clone the repo
     - Set `SOURCE="${TMPDIR}/agentic/claude-code"`
   - If `$ARGUMENTS` is a path: set `SOURCE="$ARGUMENTS/claude-code"`
   - Verify SOURCE exists; abort with a clear message if not

2. **Update agent definitions**
   - Create `.claude/agents/` in the current directory if it does not exist
   - Copy from SOURCE to `.claude/agents/`, **overwriting** any file that already exists:
     `architect.md`, `challenger.md`, `code-analyst.md`, `code-quality-auditor.md`,
     `data-analyst.md`, `dev.md`, `documentation-writer.md`, `qa.md`

3. **Clean up** the temp clone directory if one was created

4. **Report** each file that was updated and whether it was newly created or replaced
