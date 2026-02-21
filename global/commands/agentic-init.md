Initialize the current project with the agentic workflow setup.

Fetches available branches from the user's `agentic` GitHub repo and presents an interactive
picker when more than one branch exists. The chosen branch is pinned to `.claude/agentic-branch`
so that `/agentic-update` knows where to pull from.

- **main/master branch**: agents are sourced from the globally-installed `~/.claude/agents/`
- **any other branch**: agents are cloned from that branch and stored locally in `.claude/agents/`

$ARGUMENTS may contain a local filesystem path to the agentic repo. If provided, the branch
picker is skipped and that checkout is used as-is.

## Steps

1. **Determine source and branch**
   - If `$ARGUMENTS` is a path:
     - Set `SOURCE="$ARGUMENTS"` and `BRANCH="local"`
     - Skip to step 3
   - Run `GH_USER=$(gh api user --jq '.login')`
   - Run `gh api repos/${GH_USER}/agentic/branches --jq '.[].name'` to get the branch list
   - Run `gh api repos/${GH_USER}/agentic --jq '.default_branch'` to identify the default branch
   - If only one branch exists: use it, skip the picker
   - If multiple branches exist: use `AskUserQuestion` to present the list and let the user pick one
   - Set `BRANCH` to the chosen branch name

2. **Clone the selected branch**
   - Run `TMPDIR=$(mktemp -d) && gh repo clone ${GH_USER}/agentic ${TMPDIR}/agentic -- --branch ${BRANCH} --depth 1`
   - Set `SOURCE="${TMPDIR}/agentic"`
   - Verify `${SOURCE}/claude-code/` exists; abort with a clear message if not

3. **Update global agent definitions** (only if BRANCH is the default branch or `$ARGUMENTS` was given)
   - Create `~/.claude/agents/` if it does not exist
   - Copy from `${SOURCE}/claude-code/` to `~/.claude/agents/`, overwriting any existing files:
     `architect.md`, `challenger.md`, `code-analyst.md`, `code-quality-auditor.md`,
     `data-analyst.md`, `dev.md`, `documentation-writer.md`, `qa.md`

4. **Create project CLAUDE.md** (skip if `CLAUDE.md` already exists in cwd)
   - Infer project name from the current directory name
   - Write the following template:
     ```
     # CLAUDE.md

     ## Project

     **<dirname>** — [description]

     @project-config.md
     ```

5. **Install project agent definitions**
   - Create `.claude/agents/` in the current directory if it does not exist
   - If BRANCH is the default branch: copy from `~/.claude/agents/` to `.claude/agents/`, skipping any file that already exists
   - If BRANCH is not the default branch: copy from `${SOURCE}/claude-code/` to `.claude/agents/`, skipping any file that already exists

6. **Pin the branch**
   - Write BRANCH to `.claude/agentic-branch` (overwrite if exists)

7. **Clean up** the temp clone directory if one was created

8. **Report** the branch that was pinned, each file created, and each skipped (already existed)
