Pull the latest agent definitions and update the current project.

Reads the pinned branch from `.claude/agentic-branch` (written by `/agentic-init`).
Defaults to the repo's default branch if no pin file exists.

- **main/master (default) branch**: updates `~/.claude/agents/` globally, then copies to `.claude/agents/`
- **any other branch**: clones that branch and updates `.claude/agents/` directly; globals are not touched

$ARGUMENTS may contain a local filesystem path to the agentic repo. If provided, that
checkout is used as-is and the branch pin is ignored.

## Steps

1. **Determine source and branch**
   - If `$ARGUMENTS` is a path:
     - Set `SOURCE="$ARGUMENTS"` and `BRANCH="local"`
     - Skip to step 3
   - Run `GH_USER=$(gh api user --jq '.login')`
   - Read `BRANCH` from `.claude/agentic-branch` if it exists
   - If `.claude/agentic-branch` does not exist: run `gh api repos/${GH_USER}/agentic --jq '.default_branch'` and use that
   - Run `TMPDIR=$(mktemp -d) && gh repo clone ${GH_USER}/agentic ${TMPDIR}/agentic -- --branch ${BRANCH} --depth 1`
   - Set `SOURCE="${TMPDIR}/agentic"`
   - Verify `${SOURCE}/claude-code/` exists; abort with a clear message if not

2. **Determine the default branch**
   - Run `DEFAULT=$(gh api repos/${GH_USER}/agentic --jq '.default_branch')`

3. **Update agent definitions**
   - If BRANCH equals DEFAULT (or `$ARGUMENTS` was given):
     - Update global agents: copy from `${SOURCE}/claude-code/` to `~/.claude/agents/`, overwriting existing files
     - Update global orchestration: copy `${SOURCE}/claude-code/orchestration-rules.md` to `~/.claude/orchestration-rules.md`
     - Update global CLAUDE.md: copy `${SOURCE}/global/CLAUDE.md` to `~/.claude/CLAUDE.md`
     - Update project agents: copy from `~/.claude/agents/` to `.claude/agents/`, overwriting existing files
     - Update project orchestration: copy `~/.claude/orchestration-rules.md` to `.claude/orchestration-rules.md`
   - If BRANCH does not equal DEFAULT:
     - Update project agents: copy from `${SOURCE}/claude-code/` to `.claude/agents/`, overwriting existing files
     - Update project orchestration: copy `${SOURCE}/claude-code/orchestration-rules.md` to `.claude/orchestration-rules.md`
     - Do not modify `~/.claude/`
   - Agent files to copy: `architect.md`, `challenger.md`, `code-analyst.md`, `code-quality-auditor.md`,
     `data-analyst.md`, `dev.md`, `documentation-writer.md`, `qa.md`

4. **Clean up** the temp clone directory if one was created

5. **Report** the branch used, each file updated, and whether it was newly created or replaced
