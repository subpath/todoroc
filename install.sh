#!/usr/bin/env sh
set -e

REPO="subpath/todoroc"
BIN="todo"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${VERSION:-}"  # leave empty to install the latest release

# ── detect OS ────────────────────────────────────────────────────────────────
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)  os="unknown-linux-gnu" ;;
  Darwin) os="apple-darwin" ;;
  *)
    echo "Unsupported OS: $OS" >&2
    exit 1
    ;;
esac

case "$ARCH" in
  x86_64)          arch="x86_64" ;;
  arm64 | aarch64) arch="aarch64" ;;
  *)
    echo "Unsupported architecture: $ARCH" >&2
    exit 1
    ;;
esac

TARGET="${arch}-${os}"

# ── resolve version ──────────────────────────────────────────────────────────
if command -v curl > /dev/null 2>&1; then
  fetch() { curl -fsSL "$1"; }
elif command -v wget > /dev/null 2>&1; then
  fetch() { wget -qO- "$1"; }
else
  echo "curl or wget is required" >&2
  exit 1
fi

if [ -z "$VERSION" ]; then
  API="https://api.github.com/repos/${REPO}/releases/latest"
  VERSION="$(fetch "$API" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
  if [ -z "$VERSION" ]; then
    echo "Could not determine latest release version" >&2
    exit 1
  fi
fi

ARCHIVE="${BIN}-${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"

# ── download & install ───────────────────────────────────────────────────────
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Downloading $BIN $VERSION for $TARGET..."

fetch "$URL" > "$TMP/$ARCHIVE"

tar -xzf "$TMP/$ARCHIVE" -C "$TMP"
chmod +x "$TMP/$BIN"

mkdir -p "$INSTALL_DIR"
mv "$TMP/$BIN" "$INSTALL_DIR/$BIN"

echo "Installed $BIN $VERSION to $INSTALL_DIR/$BIN"

# ── PATH reminder ────────────────────────────────────────────────────────────
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    echo ""
    echo "NOTE: $INSTALL_DIR is not in your PATH."
    echo "Add this to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
    echo ""
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    ;;
esac

# ── model setup ──────────────────────────────────────────────────────────────
echo ""
echo "One-time model setup required before first use:"
echo ""
echo "  $BIN --model sentence-transformers/all-MiniLM-L6-v2"
echo "  $BIN --compile-model"
echo ""
echo "This downloads and compiles the embedding model (~25 MB)."
