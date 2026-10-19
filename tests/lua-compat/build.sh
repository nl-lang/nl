#!/bin/sh
# Builds the four interpreters the compatibility suite compares, under
# target/lua-compat:
#   stock-jit, stock-nojit   the pinned commit of the LuaJIT submodule built
#                            with LUAJIT_DISABLE_NL (git archive HEAD:
#                            independent of the working tree's changes),
#                            which is upstream LuaJIT
#   fork-jit, fork-nojit     the fork's working tree (tests/vm/build-vm.sh);
#                            fork-real-jit, fork-real-nojit with the real
#                            runtime
# The fork is built with the runtime chosen by NL_COMPAT_RUNTIME:
#   fake (default)  tests/vm/fake_runtime: the VM changes plus an opened
#                   runtime that installs a few globals
#   real            runtime/*.lua, which must open
# and with the identity compiler (no .nl file is involved here).
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
T=$ROOT/target/lua-compat
mkdir -p "$T"
case "$(uname -s)" in
  Darwin) : "${MACOSX_DEPLOYMENT_TARGET:=11.0}"; export MACOSX_DEPLOYMENT_TARGET ;;
esac
JOBS=$( (sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4) | head -1)
PIN=$(git -C "$ROOT/LuaJIT" rev-parse HEAD)

for v in jit nojit; do
  if [ $v = nojit ]; then X="-DLUAJIT_DISABLE_JIT"; else X=""; fi
  d=$T/stock-$v
  if [ "$(cat "$d/.pinned" 2>/dev/null)" != "$PIN" ] || [ ! -x "$d/src/luajit" ]; then
    echo "-- build stock-$v at ${PIN%"${PIN#???????}"}"
    mkdir -p "$d"
    git -C "$ROOT/LuaJIT" archive "$PIN" | tar -x -C "$d"
    # The pinned commit is the fork's: with NL disabled its tree builds the
    # stock interpreter (every change is behind LJ_NL, lj_arch.h).
    make -C "$d/src" -j"$JOBS" "XCFLAGS=$X -DLUAJIT_DISABLE_NL" >"$T/stock-$v.log" 2>&1 ||
      { tail -20 "$T/stock-$v.log"; exit 1; }
    echo "$PIN" >"$d/.pinned"
  fi
done

CC=${CC:-cc}
mkdir -p "$ROOT/target/vm"
# The identity compiler with the stubs of the text and regex libraries
# (LuaJIT/src/lj_nlnotext.c), as tests/vm/run.sh builds it.
$CC -O2 -fPIC -I"$ROOT/LuaJIT/src" -c "$ROOT/tests/vm/support/nlc_identity.c" \
  -o "$ROOT/target/vm/nlc_identity.o"
$CC -O2 -fPIC -I"$ROOT/LuaJIT/src" -c "$ROOT/LuaJIT/src/lj_nlnotext.c" \
  -o "$ROOT/target/vm/nlc_identity_notext.o"
ar rcs "$ROOT/target/vm/libnlc_identity.a" "$ROOT/target/vm/nlc_identity.o" \
  "$ROOT/target/vm/nlc_identity_notext.o"
case "${NL_COMPAT_RUNTIME:-fake}" in
  fake) RT=$ROOT/tests/vm/fake_runtime; FORK=fork ;;
  real) RT=$ROOT/runtime; FORK=fork-real ;;
  *) echo "NL_COMPAT_RUNTIME must be fake or real" >&2; exit 2 ;;
esac
for v in jit nojit; do
  if [ $v = nojit ]; then X="-DLUAJIT_DISABLE_JIT"; else X=""; fi
  echo "-- build $FORK-$v (runtime: ${NL_COMPAT_RUNTIME:-fake})"
  "$ROOT/tests/vm/build-vm.sh" "$T/$FORK-$v" "XCFLAGS=$X" "NL_RUNTIME_DIR=$RT" \
    "NL_LIBNLC=$ROOT/target/vm/libnlc_identity.a" >"$T/$FORK-$v.log" 2>&1 ||
    { tail -20 "$T/$FORK-$v.log"; exit 1; }
  "$T/$FORK-$v/src/luajit" -e 'assert(event and event.run)' ||
    { echo "$FORK-$v does not start"; exit 1; }
done
