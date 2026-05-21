#!/usr/bin/env bash
# knogg installer — downloads latest release from GitHub.
# Usage: curl -fsSL https://raw.githubusercontent.com/CoffeJeanCode/knogg/main/scripts/install.sh | bash
set -euo pipefail

REPO="CoffeJeanCode/knogg"
INSTALL_DIR="${KNOGG_INSTALL_DIR:-$HOME/.local/bin}"
GITHUB_URL="https://github.com/${REPO}"

detect_os_arch() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"
  case "$os" in
    linux)  os="linux" ;;
    darwin) os="macos" ;;
    *) echo "error: unsupported OS: $os" >&2; exit 1 ;;
  esac
  case "$arch" in
    x86_64|amd64)  arch="amd64" ;;
    aarch64|arm64) arch="arm64" ;;
    *) echo "error: unsupported arch: $arch" >&2; exit 1 ;;
  esac
  echo "${os}-${arch}"
}

info()  { echo "==> $*"; }
error() { echo "!!> $*" >&2; }

# Detect platform
PLATFORM="$(detect_os_arch)"
info "detected platform: ${PLATFORM}"

# Determine binary name
BINARY="knogg"
if [[ "$PLATFORM" == *"windows"* ]]; then
  BINARY="knogg.exe"
fi

ASSET_NAME="knogg-${PLATFORM}"
if [[ "$PLATFORM" == *"windows"* ]]; then
  ASSET_NAME="${ASSET_NAME}.exe"
fi

# Fetch latest release info
info "fetching latest release from ${GITHUB_URL}"
LATEST_URL="${GITHUB_URL}/releases/latest"

# Use GitHub API if curl supports it, otherwise parse HTML
if command -v jq &>/dev/null; then
  API_URL="https://api.github.com/repos/${REPO}/releases/latest"
  DOWNLOAD_URL="$(curl -fsSL "$API_URL" | jq -r ".assets[] | select(.name == \"${ASSET_NAME}\") | .browser_download_url")"
else
  DOWNLOAD_URL="$(curl -fsSL "$LATEST_URL" | grep -o "href=\"[^\"]*${ASSET_NAME}\"" | head -1 | sed 's/href="//;s/"//')"
  if [[ -z "$DOWNLOAD_URL" ]]; then
    # Try direct pattern
    TAG="$(curl -fsSL "$LATEST_URL" | grep -o 'releases/tag/[^"]*' | head -1 | sed 's/releases\/tag\///')"
    DOWNLOAD_URL="${GITHUB_URL}/releases/download/${TAG}/${ASSET_NAME}"
  fi
fi

if [[ -z "$DOWNLOAD_URL" ]]; then
  error "no release asset found for ${ASSET_NAME}"
  error "available releases: ${GITHUB_URL}/releases"
  exit 1
fi

info "downloading from ${DOWNLOAD_URL}"

# Create install directory
mkdir -p "$INSTALL_DIR"

# Download binary
TMPFILE="$(mktemp)"
curl -fsSL -o "$TMPFILE" "$DOWNLOAD_URL"
chmod +x "$TMPFILE"

# Install
mv -f "$TMPFILE" "${INSTALL_DIR}/${BINARY}"

info "installed to ${INSTALL_DIR}/${BINARY}"

# Verify
VERSION="$("${INSTALL_DIR}/${BINARY}" --version 2>/dev/null || echo "unknown")"
info "knogg ${VERSION}"

# Check PATH
if [[ ":${PATH}:" != *":${INSTALL_DIR}:"* ]]; then
  echo ""
  echo "WARNING: ${INSTALL_DIR} is not in your PATH."
  echo "Add it by running:"
  echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
  echo ""
  echo "Or add to your shell profile (~/.bashrc, ~/.zshrc):"
  echo "  echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.bashrc"
fi

echo ""
echo "Quick start:"
echo "  cd your-project"
echo "  knogg init"
echo "  knogg status"
