#!/usr/bin/env bash
# /etc/claudebox/shutdown.sh — baked into kernel image
# Executed inside VM via SSH by claudebox stop
set -euo pipefail

CLAUDEBOX_RVF="/workspace/.claudebox/project.rvf"

# Write updated META_SEG with current session state
claudebox-meta-save "$CLAUDEBOX_RVF"
