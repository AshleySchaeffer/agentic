#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

echo "Building orchestrator..."
cargo build --release

BINARY="target/release/orchestrator"
if [[ ! -f "$BINARY" ]]; then
    echo "error: build failed, binary not found at $BINARY" >&2
    exit 1
fi

# Install binary
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$INSTALL_DIR"
cp "$BINARY" "$INSTALL_DIR/orchestrator"
echo "Installed orchestrator to $INSTALL_DIR/orchestrator"

# Ensure install dir is on PATH
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
    echo ""
    echo "warning: $INSTALL_DIR is not on your PATH"
    echo "Add this to your shell profile:"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
fi

# Register hooks in Claude Code
echo ""
"$INSTALL_DIR/orchestrator" install
