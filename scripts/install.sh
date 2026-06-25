#!/bin/sh
# Sara installer — downloads the latest prebuilt binary for this OS/arch from
# GitHub Releases and installs it. Usage:
#   curl -fsSL https://raw.githubusercontent.com/Abarbesgaard/Sara/main/scripts/install.sh | sh
#
# Env:
#   SARA_INSTALL_DIR   target dir (default: ~/.local/bin)
#   SARA_VERSION       tag to install (default: latest)
set -eu

REPO="Abarbesgaard/Sara"
BIN="sara"
INSTALL_DIR="${SARA_INSTALL_DIR:-$HOME/.local/bin}"

err() { printf 'error: %s\n' "$1" >&2; exit 1; }

# Detect platform → release target triple (matches release.yml archive names).
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
    Linux)  os_part="unknown-linux-gnu" ;;
    Darwin) os_part="apple-darwin" ;;
    *) err "unsupported OS: $os (try: cargo install sara-tasks)" ;;
esac
case "$arch" in
    x86_64|amd64)   arch_part="x86_64" ;;
    aarch64|arm64)  arch_part="aarch64" ;;
    *) err "unsupported arch: $arch (try: cargo install sara-tasks)" ;;
esac
target="${arch_part}-${os_part}"

# Resolve version.
version="${SARA_VERSION:-latest}"
if [ "$version" = "latest" ]; then
    base="https://github.com/${REPO}/releases/latest/download"
else
    base="https://github.com/${REPO}/releases/download/${version}"
fi

asset="${BIN}-${target}.tar.gz"
url="${base}/${asset}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

printf 'Downloading %s ...\n' "$url"
if ! curl -fsSL "$url" -o "$tmp/$asset"; then
    err "download failed. The release may not have an asset for $target yet — try: cargo install sara-tasks"
fi

tar -xzf "$tmp/$asset" -C "$tmp"
# The binary may sit at the archive root or inside a dir; find it.
binpath="$(find "$tmp" -type f -name "$BIN" -perm -u+x 2>/dev/null | head -n1)"
[ -z "$binpath" ] && binpath="$(find "$tmp" -type f -name "$BIN" | head -n1)"
[ -z "$binpath" ] && err "could not find '$BIN' inside the archive"

mkdir -p "$INSTALL_DIR"
install -m 0755 "$binpath" "$INSTALL_DIR/$BIN"

printf '\nInstalled %s to %s\n' "$BIN" "$INSTALL_DIR/$BIN"
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) printf '\nNote: %s is not on your PATH. Add it:\n  export PATH="%s:$PATH"\n' "$INSTALL_DIR" "$INSTALL_DIR" ;;
esac
"$INSTALL_DIR/$BIN" --version 2>/dev/null || true
