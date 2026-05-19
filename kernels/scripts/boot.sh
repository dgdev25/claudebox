#!/usr/bin/env bash
# /etc/claudebox/boot.sh — baked into kernel image
# Executed inside VM via SSH by claudebox start (step 16)
set -euo pipefail

CLAUDEBOX_RVF="/workspace/.claudebox/project.rvf"
SESSION_CTX="/run/claudebox/session-context.txt"

mkdir -p "$(dirname "$SESSION_CTX")"

# Read META_SEG and write session context for Claude Code
claudebox-meta-export "$CLAUDEBOX_RVF" > "$SESSION_CTX"

# Validate and export MCP port for claudebox-mcp
_port="${CLAUDEBOX_MCP_PORT:-7878}"
if ! [[ "$_port" =~ ^[0-9]+$ ]] || [ "$_port" -lt 1 ] || [ "$_port" -gt 65535 ]; then
    echo "Invalid CLAUDEBOX_MCP_PORT: ${_port}" >&2
    exit 1
fi
export CLAUDEBOX_MCP_PORT="$_port"
