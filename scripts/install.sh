#!/usr/bin/env bash
set -euo pipefail

# Tetonic / Lokai Installer for macOS and Linux
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/tetonic-labs/tetonic/main/scripts/install.sh | bash

REPO="${TETONIC_REPO:-tetonic-labs/tetonic}"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# Fallback to ~/.local/bin if /usr/local/bin is not writable
if [ ! -w "$INSTALL_DIR" ]; then
  INSTALL_DIR="$HOME/.local/bin"
  mkdir -p "$INSTALL_DIR"
fi

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64)
        ASSET="lokai-darwin-arm64.tar.gz"
        ;;
      x86_64)
        ASSET="lokai-darwin-x64.tar.gz"
        ;;
      *)
        echo "Error: Unsupported architecture $ARCH on macOS." >&2
        exit 1
        ;;
    esac
    ;;
  Linux)
    case "$ARCH" in
      x86_64)
        ASSET="lokai-linux-x64.tar.gz"
        ;;
      aarch64|arm64)
        ASSET="lokai-linux-arm64.tar.gz"
        ;;
      *)
        echo "Error: Unsupported architecture $ARCH on Linux." >&2
        exit 1
        ;;
    esac
    ;;
  *)
    echo "Error: Unsupported operating system $OS. For Windows, use install.ps1." >&2
    exit 1
    ;;
esac

RELEASE_BASE_URL="https://github.com/${REPO}/releases/latest/download"
DOWNLOAD_URL="${RELEASE_BASE_URL}/${ASSET}"
CHECKSUM_URL="${RELEASE_BASE_URL}/checksums.txt"

TMP_DIR="$(mktemp -d -t tetonic-install.XXXXXX)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo "==> Downloading ${ASSET} from ${DOWNLOAD_URL}..."
curl -fsSL "$DOWNLOAD_URL" -o "${TMP_DIR}/${ASSET}"

echo "==> Downloading checksums..."
curl -fsSL "$CHECKSUM_URL" -o "${TMP_DIR}/checksums.txt"

echo "==> Verifying SHA256 checksum..."
EXPECTED_SHA=$(grep "${ASSET}" "${TMP_DIR}/checksums.txt" | awk '{print $1}')
if [ -z "$EXPECTED_SHA" ]; then
  echo "Warning: Checksum for ${ASSET} not found in checksums.txt. Proceeding with caution."
else
  if command -v shasum >/dev/null 2>&1; then
    ACTUAL_SHA=$(shasum -a 256 "${TMP_DIR}/${ASSET}" | awk '{print $1}')
  else
    ACTUAL_SHA=$(sha256sum "${TMP_DIR}/${ASSET}" | awk '{print $1}')
  fi

  if [ "$EXPECTED_SHA" != "$ACTUAL_SHA" ]; then
    echo "Error: Checksum verification failed!" >&2
    echo "Expected: $EXPECTED_SHA" >&2
    echo "Actual:   $ACTUAL_SHA" >&2
    exit 1
  fi
  echo "Checksum verified: ${ACTUAL_SHA}"
fi

echo "==> Extracting binaries..."
tar -xzf "${TMP_DIR}/${ASSET}" -C "$TMP_DIR"

echo "==> Installing binaries to ${INSTALL_DIR}..."
cp "${TMP_DIR}/lokai" "${INSTALL_DIR}/lokai"
cp "${TMP_DIR}/lokaid" "${INSTALL_DIR}/lokaid"
chmod +x "${INSTALL_DIR}/lokai" "${INSTALL_DIR}/lokaid"

echo ""
echo "=========================================================="
echo "  Lokai installed successfully to ${INSTALL_DIR}/lokai"
echo "=========================================================="
echo ""

if ! command -v lokai >/dev/null 2>&1; then
  echo "Note: ${INSTALL_DIR} is not in your PATH."
  echo "Add the following line to your shell profile (~/.bashrc or ~/.zshrc):"
  echo "  export PATH=\"\$PATH:${INSTALL_DIR}\""
  echo ""
fi

echo "Run 'lokai --help' to get started."
