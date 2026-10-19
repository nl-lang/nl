#!/bin/sh
# Brings the luajit-src-rs submodule in step with this repository, so that a
# Rust program patching luajit-src with it builds the VM and runtime that
# this commit expects:
#
#   - its luajit2/ is the fork at the commit checked out in LuaJIT/;
#   - its luajit_relver.txt is that commit's time, which LuaJIT puts in its
#     version string;
#   - its version's build metadata names that commit;
#   - its nl-runtime/ is a copy of runtime/*.lua.
#
#   tests/vm/support/sync_luajit_src_rs.sh [--check]
#
# Then commit in luajit-src-rs, and the submodule here. --check changes
# nothing, names what differs, and exits 1 if anything does.
set -eu
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
L=$ROOT/luajit-src-rs
CHECK=${1:-}
[ -f "$L/Cargo.toml" ] ||
  { echo "sync_luajit_src_rs: luajit-src-rs is not checked out (git submodule update --init luajit-src-rs)" >&2; exit 1; }

FORK=$(git -C "$ROOT/LuaJIT" rev-parse HEAD)
TIME=$(git -C "$ROOT/LuaJIT" show -s --format=%ct HEAD)
SHORT=$(printf %.7s "$FORK")
VERSION=$(sed -n 's/^version = "\([^+"]*\).*"$/\1/p' "$L/Cargo.toml" | head -1)
DIFF=0
differs() { echo "luajit-src-rs: $1"; DIFF=1; }

LINK=$(git -C "$L" ls-files -s luajit2 | cut -d' ' -f2)
[ "$LINK" = "$FORK" ] || differs "luajit2 is $LINK, LuaJIT is $FORK"
[ "$(cat "$L/luajit_relver.txt")" = "$TIME" ] || differs "luajit_relver.txt is not $TIME"
grep -q "^version = \"$VERSION+$SHORT\"$" "$L/Cargo.toml" || differs "version is not $VERSION+$SHORT"
for f in "$ROOT"/runtime/*.lua; do
  cmp -s "$f" "$L/nl-runtime/${f##*/}" || differs "nl-runtime/${f##*/} differs from runtime/"
done
for f in "$L"/nl-runtime/*.lua; do
  [ -e "$f" ] || continue
  [ -e "$ROOT/runtime/${f##*/}" ] || differs "nl-runtime/${f##*/} is not in runtime/"
done

if [ "$CHECK" = --check ] || [ $DIFF = 0 ]; then exit $DIFF; fi

if [ -e "$L/luajit2/.git" ]; then
  git -C "$L/luajit2" fetch -q "$ROOT/LuaJIT" "$FORK"
  git -C "$L/luajit2" checkout -q --detach "$FORK"
  git -C "$L" add luajit2
else
  git -C "$L" update-index --cacheinfo "160000,$FORK,luajit2"
fi
echo "$TIME" >"$L/luajit_relver.txt"
sed -e "s/^version = \"$VERSION+[^\"]*\"$/version = \"$VERSION+$SHORT\"/" "$L/Cargo.toml" >"$L/Cargo.toml.tmp"
mv -f "$L/Cargo.toml.tmp" "$L/Cargo.toml"
mkdir -p "$L/nl-runtime"
rm -f "$L"/nl-runtime/*.lua
cp "$ROOT"/runtime/*.lua "$L/nl-runtime/"
git -C "$L" add -A luajit_relver.txt Cargo.toml nl-runtime
echo "luajit-src-rs: updated to the fork at $SHORT and runtime/; commit it, then the submodule here"
