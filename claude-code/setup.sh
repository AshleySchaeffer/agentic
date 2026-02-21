#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

mkdir -p ~/.claude/agents ~/.claude/commands

# Global CLAUDE.md — skip if exists (user may have customised)
if [[ -f ~/.claude/CLAUDE.md ]]; then
    echo "skip  ~/.claude/CLAUDE.md (already exists)"
else
    cp "${REPO_DIR}/global/CLAUDE.md" ~/.claude/CLAUDE.md
    echo "ok    ~/.claude/CLAUDE.md"
fi

# Orchestration rules
cp "${REPO_DIR}/claude-code/orchestration-rules.md" ~/.claude/orchestration-rules.md
echo "ok    ~/.claude/orchestration-rules.md"

# Agent definitions
for agent in architect challenger code-analyst code-quality-auditor data-analyst dev documentation-writer qa; do
    cp "${REPO_DIR}/claude-code/${agent}.md" ~/.claude/agents/${agent}.md
    echo "ok    ~/.claude/agents/${agent}.md"
done

# Commands
for cmd in "${REPO_DIR}/global/commands/"*.md; do
    name="$(basename "$cmd")"
    cp "$cmd" ~/.claude/commands/${name}
    echo "ok    ~/.claude/commands/${name}"
done

echo ""
echo "Done. Restart Claude Code to pick up the changes."
