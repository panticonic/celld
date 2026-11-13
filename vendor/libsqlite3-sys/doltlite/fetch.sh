#!/bin/sh
# Fetches the pinned DoltLite release amalgamation tarball and unpacks it into
# $1 (default: ./src next to this script). Idempotent. The sqlite3.c inside is
# ~21 MB and is not committed; build.rs calls this when the directory is absent.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
dest=${1:-$here/src}
version=0.51.11
sha256=b88e2192a7fb0e0e04502556ec599799a510aa93385ef4f8bb002adcb22dde01
url=https://github.com/dolthub/doltlite/releases/download/v$version/doltlite-autoconf-$version.tar.gz

if [ -f "$dest/sqlite3.c" ] && [ "$(cat "$dest/.doltlite_release_version" 2>/dev/null)" = "$version" ]; then
  exit 0
fi
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL -o "$tmp/src.tar.gz" "$url"
echo "$sha256  $tmp/src.tar.gz" | sha256sum -c - >&2
mkdir -p "$tmp/x"
tar xzf "$tmp/src.tar.gz" -C "$tmp/x"
rm -rf "$dest"
mkdir -p "$(dirname "$dest")"
mv "$tmp/x/doltlite-autoconf-$version" "$dest"
echo "$version" > "$dest/.doltlite_release_version"
