#!/bin/sh
# Prepares a luajit-src crate that builds the VM fork, for Rust programs
# outside the workspace that link mlua against it through
#
#   [patch.crates-io]
#   luajit-src = { path = "DEST" }
#
#   tests/vm/support/prepare_luajit_src.sh DEST [--nojit]
#
# --nojit builds the VM with -DLUAJIT_DISABLE_JIT: luajit-src sets XCFLAGS
# itself and runs make -e, so the copied Makefile adds the flag with
# `override`.
#
# DEST is a copy of the luajit-src-rs submodule (its build logic, its
# manifest without its workspace), whose luajit2/ is the fork's working tree
# (tracked and untracked files, minus ignored ones, as tests/vm/build-vm.sh
# copies it) and whose nl-runtime/ holds runtime/*.lua: the working trees,
# not the commits luajit-src-rs names, so local changes are what gets built.
# The crate builds the VM with nl-runtime/ as NL_RUNTIME_DIR, unless the
# environment sets one, as tests/vm/nl_sys_link does for the fake runtime.
# Nothing in the repository's git state changes.
#
# Cargo does not see these files: luajit-src has no build script, so its
# fingerprint is its src/lib.rs alone, and mlua-sys's build script, which
# runs the LuaJIT build, reruns only when luajit-src is rebuilt. So when
# anything here changed, src/lib.rs is touched; when nothing did, no file's
# timestamp changes and cargo reuses the LuaJIT it built.
set -eu
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
DEST=$1
NOJIT=${2:-}
mkdir -p "$DEST/luajit2" "$DEST/nl-runtime" "$DEST/src"

CHANGED=$DEST/.changed.$$
: >"$CHANGED"
trap 'rm -f "$CHANGED"' EXIT

# Writes stdin to $1 unless $1 already has that content.
put() {
  tmp=$1.tmp.$$
  cat >"$tmp"
  if cmp -s "$tmp" "$1"; then rm -f "$tmp"; else mv -f "$tmp" "$1"; echo "$1" >>"$CHANGED"; fi
}
# rsync, recording what it changed.
sync() {
  rsync -aiO --checksum "$@" >>"$CHANGED"
}

L=$ROOT/luajit-src-rs
[ -f "$L/src/lib.rs" ] ||
  { echo "prepare_luajit_src: luajit-src-rs is not checked out (git submodule update --init luajit-src-rs)" >&2; exit 1; }

put "$DEST/src/lib.rs" <"$L/src/lib.rs"
for f in README.md LICENSE; do
  sync "$L/$f" "$DEST/$f"
done
sed -e '/^\[workspace\]/,/^$/d' "$L/Cargo.toml" | put "$DEST/Cargo.toml"
# The crate copies luajit_relver.txt to luajit2/.relver: the time of the
# fork's checked-out commit, as tests/vm/support/sync_luajit_src_rs.sh
# records it.
git -C "$ROOT/LuaJIT" show -s --format=%ct HEAD | put "$DEST/luajit_relver.txt"

(cd "$ROOT/LuaJIT" && git ls-files -co --exclude-standard) |
  while IFS= read -r f; do
    case $f in .relver|src/Makefile) continue ;; esac  # from luajit_relver.txt; below
    [ -f "$ROOT/LuaJIT/$f" ] && printf '%s\n' "$f"
  done |
  sync --files-from=- "$ROOT/LuaJIT/" "$DEST/luajit2/"
# The flag goes right after XCFLAGS is set: the Makefile reads XCFLAGS with
# := further down (the target's architecture test).
if [ "$NOJIT" = --nojit ]; then NJ='/^XCFLAGS=$/a\
override XCFLAGS+= -DLUAJIT_DISABLE_JIT
'; else NJ=''; fi
sed -e "$NJ" "$ROOT/LuaJIT/src/Makefile" | put "$DEST/luajit2/src/Makefile"
sync --delete --include='*.lua' --exclude='*' "$ROOT/runtime/" "$DEST/nl-runtime/"
if [ -s "$CHANGED" ]; then touch "$DEST/src/lib.rs"; fi
