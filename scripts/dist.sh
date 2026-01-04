#!/usr/bin/env bash
# Build static Linux binary using musl
set -euo pipefail

VERSION=0.1.0
TMP="$(mktemp -d)"
DESTDIR="$TMP/notifiers-$VERSION-linux-x86_64"
mkdir -p "$DESTDIR"
nix build .#x86_64
cp result/bin/notifiers "$DESTDIR"
pushd "$TMP"
tar czf "notifiers-$VERSION-linux-x86_64.tar.gz" "notifiers-$VERSION-linux-x86_64"
popd
mkdir -p dist
mv "$TMP/notifiers-$VERSION-linux-x86_64.tar.gz" "dist/notifiers-$VERSION-linux-x86_64.tar.gz"
rm -fr "$TMP"
