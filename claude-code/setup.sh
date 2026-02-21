#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

mkdir -p ~/.claude/commands

# Commands
for cmd in "${REPO_DIR}/global/commands/"*.md; do
    name="$(basename "$cmd")"
    cp "$cmd" ~/.claude/commands/${name}
    echo "ok    ~/.claude/commands/${name}"
done

echo ""
echo "Done. Run /agentic-setup in Claude Code to complete setup."
