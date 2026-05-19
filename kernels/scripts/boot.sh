#!/usr/bin/env bash
# /etc/claudebox/boot.sh — baked into kernel image
# Executed inside VM via SSH by claudebox start (step 16)
set -euo pipefail

CLAUDEBOX_RVF="/workspace/.claudebox/project.rvf"
SESSION_CTX="/run/claudebox/session-context.txt"

mkdir -p "$(dirname "$SESSION_CTX")"

# Read META_SEG and write session context for Claude Code
claudebox-meta-export "$CLAUDEBOX_RVF" > "$SESSION_CTX"

# Export MCP port for claudebox-mcp
export CLAUDEBOX_MCP_PORT="${CLAUDEBOX_MCP_PORT:-7878}"
