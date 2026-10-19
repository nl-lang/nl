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
# DEST is a copy of luajit-src-rs (its manifest without its workspace, its
# build logic), whose luajit2/ is the fork's working tree (tracked and
# untracked files, minus ignored ones, as tests/vm/build-vm.sh copies it),
# and whose luajit2/nl-runtime/ holds the runtime library (runtime/*.lua).
# The copied src/Makefile names that directory as the default of
# NL_RUNTIME_DIR, so a crate needs no environment to build the real runtime;
# NL_RUNTIME_DIR in the environment still wins (luajit-src runs make -e), as
# tests/vm/nl_sys_link uses it for the fake runtime. Nothing in the
# repository's git state changes.
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
mkdir -p "$DEST/luajit2/nl-runtime" "$DEST/src"

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

put "$DEST/src/lib.rs" <"$ROOT/luajit-src-rs/src/lib.rs"
for f in luajit_relver.txt README.md LICENSE; do
  sync "$ROOT/luajit-src-rs/$f" "$DEST/$f"
done
# mlua-sys 0.13 (mlua 0.12) takes luajit-src 210.7, the version of the fork
# of luajit-src-rs, so its manifest is used as it is.
sed -e '/^\[workspace\]/,/^$/d' "$ROOT/luajit-src-rs/Cargo.toml" | put "$DEST/Cargo.toml"

(cd "$ROOT/LuaJIT" && git ls-files -co --exclude-standard) |
  while IFS= read -r f; do
    case $f in .relver|src/Makefile) continue ;; esac  # written below
    [ -f "$ROOT/LuaJIT/$f" ] && printf '%s\n' "$f"
  done |
  sync --files-from=- "$ROOT/LuaJIT/" "$DEST/luajit2/"
git -C "$ROOT/LuaJIT" show -s --format=%ct HEAD | put "$DEST/luajit2/.relver"
# make runs in a copy of luajit2/src, so ../nl-runtime is luajit2/nl-runtime.
# The flag goes right after XCFLAGS is set: the Makefile reads XCFLAGS with
# := further down (the target's architecture test).
if [ "$NOJIT" = --nojit ]; then NJ='/^XCFLAGS=$/a\
override XCFLAGS+= -DLUAJIT_DISABLE_JIT
'; else NJ=''; fi
sed -e 's#^NL_RUNTIME_DIR= \.\./\.\./runtime$#NL_RUNTIME_DIR= ../nl-runtime#' -e "$NJ" \
  "$ROOT/LuaJIT/src/Makefile" | put "$DEST/luajit2/src/Makefile"
grep -q '^NL_RUNTIME_DIR= \.\./nl-runtime$' "$DEST/luajit2/src/Makefile" ||
  { echo "prepare_luajit_src: the fork's Makefile no longer sets NL_RUNTIME_DIR= ../../runtime" >&2; exit 1; }
sync --delete --include='*.lua' --exclude='*' "$ROOT/runtime/" "$DEST/luajit2/nl-runtime/"
if [ -s "$CHANGED" ]; then touch "$DEST/src/lib.rs"; fi
