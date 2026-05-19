#!/usr/bin/env bash
# claudebox installer
# Usage: curl -fsSL https://raw.githubusercontent.com/dgdev25/claudebox/main/install.sh | bash
set -euo pipefail

REPO="${CLAUDEBOX_REPO:-dgdev25/claudebox}"
INSTALL_DIR="${CLAUDEBOX_INSTALL_DIR:-/usr/local/bin}"
BIN="claudebox"

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}" in
  Darwin)
    case "${ARCH}" in
      arm64)  TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *) echo "Unsupported macOS arch: ${ARCH}" >&2; exit 1 ;;
    esac
    ;;
  Linux)
    case "${ARCH}" in
      x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
      *) echo "Unsupported Linux arch: ${ARCH}" >&2; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: ${OS}" >&2
    exit 1
    ;;
esac

# Resolve latest release tag
echo "Fetching latest claudebox release from ${REPO}..."
LATEST_JSON=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" || true)
if [ -z "${LATEST_JSON}" ]; then
  echo "Could not fetch releases for ${REPO}." >&2
  echo "If this repository is private or incorrect, set CLAUDEBOX_REPO=owner/repo and retry." >&2
  exit 1
fi

LATEST=$(printf '%s' "${LATEST_JSON}" \
  | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')

if [ -z "$LATEST" ]; then
  echo "Could not determine latest release tag. Check https://github.com/${REPO}/releases" >&2
  exit 1
fi

echo "Installing claudebox ${LATEST} (${TARGET})..."

DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST}/claudebox-${TARGET}"
TMP="$(mktemp)"
if ! curl -fL --progress-bar -o "$TMP" "$DOWNLOAD_URL"; then
  echo "" >&2
  echo "Failed to download binary asset: ${DOWNLOAD_URL}" >&2
  echo "Verify release ${LATEST} contains claudebox-${TARGET}." >&2
  exit 1
fi
chmod +x "$TMP"

# Install — try INSTALL_DIR directly, fall back to sudo
if [ -w "$INSTALL_DIR" ]; then
  mv "$TMP" "${INSTALL_DIR}/${BIN}"
else
  echo "Writing to ${INSTALL_DIR} requires sudo..."
  sudo mv "$TMP" "${INSTALL_DIR}/${BIN}"
fi

echo ""
echo "claudebox ${LATEST} installed to ${INSTALL_DIR}/${BIN}"
echo ""
echo "Next step:"
echo "  claudebox setup"
