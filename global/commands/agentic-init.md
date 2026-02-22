Initialize or re-initialize the current project with the agentic workflow setup.

Can be run on a fresh project or re-run at any time to switch branches. Fetches available
branches from the user's `agentic` GitHub repo and presents an interactive picker when more
than one branch exists. If `.claude/agentic-branch` already exists, the current branch is
highlighted in the picker. The chosen branch is pinned to `.claude/agentic-branch` so that
`/agentic-update` knows where to pull from.

- **main/master branch**: globals (`~/.claude/agents/`) are updated and agents are copied from there
- **any other branch**: agents are cloned from that branch directly into `.claude/agents/`; globals are not touched

`CLAUDE.md` is never overwritten — on re-init, missing references are merged in without touching existing content. `project.md` is scaffolded if absent but never overwritten. Agent definition files are always overwritten (including on re-init).

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
   - Read the current pin from `.claude/agentic-branch` if it exists (for display only)
   - If only one branch exists: use it, skip the picker
   - If multiple branches exist: use `AskUserQuestion` to present the list; label the currently pinned branch with `(current)` if one is set

2. **Clone the selected branch**
   - Run `TMPDIR=$(mktemp -d) && gh repo clone ${GH_USER}/agentic ${TMPDIR}/agentic -- --branch ${BRANCH} --depth 1`
   - Set `SOURCE="${TMPDIR}/agentic"`
   - Verify `${SOURCE}/claude-code/` exists; abort with a clear message if not

3. **Update global agent definitions** (only if BRANCH is the default branch)
   - Create `~/.claude/agents/` if it does not exist
   - Copy from `${SOURCE}/claude-code/` to `~/.claude/agents/`, overwriting any existing files:
     `architect.md`, `challenger.md`, `code-analyst.md`, `code-quality-auditor.md`,
     `data-analyst.md`, `dev.md`, `documentation-writer.md`, `qa.md`

4. **Ensure project CLAUDE.md**
   - If `CLAUDE.md` does not exist in cwd:
     - Infer project name from the current directory name
     - Write the following template:
       ```
       # CLAUDE.md

       ## Project

       **<dirname>** — [description]

       @project-config.md
       ```
   - If `CLAUDE.md` already exists:
     - Read the file
     - Check for required references: `@project-config.md`
     - For each missing reference, append it on its own line at the end of the file
     - Do not modify any existing content

5. **Scaffold project.md** (skip if `project.md` already exists in cwd)
   - Write the following template:
     ```
     # Project

     ## Description

     [One-paragraph description of what this project does and who it serves.]

     ## Requirements

     [Key product requirements, user stories, or acceptance criteria.]
     ```

6. **Install project agent definitions** (always overwrite)
   - Create `.claude/agents/` in the current directory if it does not exist
   - If BRANCH is the default branch: copy from `~/.claude/agents/` to `.claude/agents/`, overwriting any existing files
   - If BRANCH is not the default branch: copy from `${SOURCE}/claude-code/` to `.claude/agents/`, overwriting any existing files
   - Files to copy: `architect.md`, `challenger.md`, `code-analyst.md`, `code-quality-auditor.md`,
     `data-analyst.md`, `dev.md`, `documentation-writer.md`, `qa.md`

7. **Configure pipeline permissions**
   - Read `.claude/settings.local.json` (treat as `{}` if it does not exist)
   - Ensure the following entries exist in `permissions.allow` (add any that are missing; preserve all existing entries):
     - `Bash(mkdir -p .claude/*)` — directory creation for agent internals
     - `Bash(rm -rf .claude/agent-internals*)` — agent state cleanup on team shutdown
     - `Bash(cp *.md *)` — markdown file copying during updates
     - `Bash(mktemp *)` — temp directory creation during repo cloning
     - `Bash(tmux *)` — tmux pane management
     - `Bash(which *)` — tool detection
     - `Write(.claude/**)` — agent state externalization, settings, and definition writes
     - `Edit(.claude/**)` — agent state and definition edits
   - Write the updated settings back to `.claude/settings.local.json`

8. **Pin the branch**
   - Write BRANCH to `.claude/agentic-branch`, overwriting any existing value

9. **Clean up** the temp clone directory if one was created

10. **Report** the branch that was pinned and each file that was installed
