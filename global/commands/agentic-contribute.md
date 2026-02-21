Contribute changes to the `claude-code/` agent definitions in the agentic repo by creating
a branch and raising a PR.

Must be run from within the agentic repo (the directory containing `claude-code/`).
$ARGUMENTS may contain a PR description; otherwise one is generated from the diff.

## Steps

1. **Verify context**
   - Confirm a `claude-code/` directory exists in the current working directory
   - Abort with a clear message if not found — this command must be run from within the agentic repo

2. **Check for changes**
   - Run `git status claude-code/` to check for modified, added, or deleted files
   - Abort with a message if there are no changes to contribute

3. **Create branch**
   - Run `git checkout -b agent-update-$(date +%Y%m%d-%H%M%S)` to create a timestamped branch

4. **Stage and commit**
   - Run `git add claude-code/`
   - Summarize what changed (list modified files) and compose a commit message describing the changes
   - If `$ARGUMENTS` is provided, use it as the commit message body; otherwise generate one from the diff
   - Commit with the message

5. **Push**
   - Run `git push -u origin <branch>` to push the branch to the remote

6. **Raise PR**
   - Run `gh pr create --title "..." --body "..."` with a title and body describing the changes
   - If `$ARGUMENTS` was provided, use it as the PR body; otherwise generate one from the diff summary

7. **Report** the PR URL
