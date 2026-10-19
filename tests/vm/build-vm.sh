#!/bin/sh
# Builds one variant of the VM fork outside the submodule's working tree.
#
#   tests/vm/build-vm.sh OUTDIR [VAR=value ...]
#
# Copies the source files of LuaJIT/ (tracked and untracked, minus ignored
# ones, as they are in the working tree) to OUTDIR, then runs make in
# OUTDIR/src with the given variables. Unchanged files keep their timestamps,
# so a second run only rebuilds what changed. NL_RUNTIME_DIR defaults to the
# repository's runtime/. Prints nothing but make's output.
set -eu

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=$1
shift
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)

# Sync the source tree.
(cd "$ROOT/LuaJIT" && git ls-files -co --exclude-standard) |
  while IFS= read -r f; do [ -f "$ROOT/LuaJIT/$f" ] && printf '%s\n' "$f"; done |
  rsync -a --checksum --files-from=- "$ROOT/LuaJIT/" "$OUT/"

# git archive would expand .relver; a copy of the working tree must do it here.
RELVER=$(git -C "$ROOT/LuaJIT" show -s --format=%ct HEAD)
if [ "$(cat "$OUT/.relver" 2>/dev/null)" != "$RELVER" ]; then
  printf '%s\n' "$RELVER" >"$OUT/.relver"
fi

case "$(uname -s)" in
  Darwin) : "${MACOSX_DEPLOYMENT_TARGET:=11.0}"; export MACOSX_DEPLOYMENT_TARGET ;;
esac
JOBS=$( (sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4) | head -1)

# The repository's runtime unless the caller names one. (Passing both would
# not do: make's recursive call for "amalg" sees them in the other order.)
case " $* " in
  *" NL_RUNTIME_DIR="*) exec make -C "$OUT/src" -j"$JOBS" "$@" ;;
  *) exec make -C "$OUT/src" -j"$JOBS" "NL_RUNTIME_DIR=$ROOT/runtime" "$@" ;;
esac
