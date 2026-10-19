#!/bin/sh
# Fetches the upstream LuaJIT test suite (LuaJIT-test-cleanup) at a pinned
# commit into target/lua-compat/LuaJIT-test-cleanup. Not vendored.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
URL=https://github.com/LuaJIT/LuaJIT-test-cleanup.git
COMMIT=014708bceb70550a3ab8d539cff14d9085ca9cb8
DIR=$ROOT/target/lua-compat/LuaJIT-test-cleanup
mkdir -p "$ROOT/target/lua-compat"
if [ ! -d "$DIR/.git" ]; then
  git clone -q "$URL" "$DIR"
fi
if [ "$(git -C "$DIR" rev-parse HEAD)" != "$COMMIT" ]; then
  git -C "$DIR" fetch -q origin
  git -C "$DIR" checkout -q --detach "$COMMIT"
fi
echo "LuaJIT-test-cleanup at $(git -C "$DIR" rev-parse --short HEAD)"
