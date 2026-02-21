Set up the agentic workflow globally for Claude Code.

Installs agent definitions, orchestration rules, and global CLAUDE.md into `~/.claude/`,
and ensures Claude team mode is enabled in `~/.claude/settings.json`.

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

2. **Install global CLAUDE.md** (skip if `~/.claude/CLAUDE.md` already exists)
   - Copy `${SOURCE}/global/CLAUDE.md` to `~/.claude/CLAUDE.md`

3. **Install orchestration rules**
   - Copy `${SOURCE}/claude-code/orchestration-rules.md` to `~/.claude/orchestration-rules.md`

4. **Install agent definitions**
   - Create `~/.claude/agents/` if it does not exist
   - Copy from `${SOURCE}/claude-code/` to `~/.claude/agents/`, overwriting any existing files:
     `architect.md`, `challenger.md`, `code-analyst.md`, `code-quality-auditor.md`,
     `data-analyst.md`, `dev.md`, `documentation-writer.md`, `qa.md`

5. **Enable team mode**
   - Read `~/.claude/settings.json` (treat as `{}` if it does not exist)
   - Ensure `env.CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` is set to `"1"`
   - Write the updated settings back to `~/.claude/settings.json`

6. **Clean up** the temp clone directory if one was created

7. **Report** each action taken and remind the user to restart Claude Code for changes to take effect
