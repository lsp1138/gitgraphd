#!/bin/sh
# Install gg (gitgraphd) from the latest GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/lsp1138/gitgraphd/main/install.sh | sh
#
#   GG_VERSION=v0.2.0      install that tag instead of the latest release
#   GG_INSTALL_DIR=/path   where to put gg (default: ~/.local/bin)
#
# Linux (x86_64, aarch64) and macOS (Intel, Apple silicon). Anything else:
# cargo install gitgraphd.
set -eu

repo="lsp1138/gitgraphd"
dir="${GG_INSTALL_DIR:-$HOME/.local/bin}"

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Linux)  os_part="unknown-linux-musl" ;;
  Darwin) os_part="apple-darwin" ;;
  *) echo "gg: no prebuilt binary for $os; try: cargo install gitgraphd" >&2; exit 1 ;;
esac
case "$arch" in
  x86_64|amd64)  arch_part="x86_64" ;;
  arm64|aarch64) arch_part="aarch64" ;;
  *) echo "gg: no prebuilt binary for $os $arch; try: cargo install gitgraphd" >&2; exit 1 ;;
esac
target="$arch_part-$os_part"
asset="gg-$target.tar.gz"

if [ -n "${GG_VERSION:-}" ]; then
  base="https://github.com/$repo/releases/download/$GG_VERSION"
else
  base="https://github.com/$repo/releases/latest/download"
fi

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q "$1" -O "$2"; }
else
  echo "gg: need curl or wget" >&2; exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "fetching $asset"
fetch "$base/$asset" "$tmp/$asset"
fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS"

want=$(grep " $asset\$" "$tmp/SHA256SUMS" | cut -d' ' -f1)
if command -v sha256sum >/dev/null 2>&1; then
  have=$(sha256sum "$tmp/$asset" | cut -d' ' -f1)
else
  have=$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)
fi
if [ -z "$want" ] || [ "$want" != "$have" ]; then
  echo "gg: checksum mismatch for $asset" >&2; exit 1
fi

tar xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$dir"
install -m 755 "$tmp/gg-$target/gg" "$dir/gg"
echo "installed $("$dir/gg" --version) to $dir/gg"

case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "note: $dir is not on your PATH" ;;
esac
