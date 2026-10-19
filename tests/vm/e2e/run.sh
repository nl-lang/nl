#!/bin/sh
# The end-to-end test with the real compiler library
# (crates/nl_capi, built with cargo) and the real runtime (runtime/*.lua),
# JIT on and off:
#   - host.c: a C host loads .nl and .lua files that import/require each other;
#   - the fork's luajit runs project/main.lua, compared with project/main.out.
# Exit status 0 when everything passed.
set -u
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
T=$ROOT/target/vm
P=$ROOT/tests/vm/e2e/project
CC=${CC:-cc}
case "$(uname -s)" in
  Darwin) : "${MACOSX_DEPLOYMENT_TARGET:=11.0}"; export MACOSX_DEPLOYMENT_TARGET
          NATIVE="" ;;
  Linux)  NATIVE="-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc" ;;
  *)      NATIVE="" ;;
esac
mkdir -p "$T"
STATUS=0

echo "-- cargo build -p nl_capi --release"
if ! cargo build -q -p nl_capi --release --manifest-path "$ROOT/Cargo.toml"; then
  echo "e2e: cannot build libnlc (crates/nl_capi)"
  exit 1
fi
LIBNLC=$ROOT/target/release/libnl_capi.a

for v in jit nojit; do
  if [ $v = nojit ]; then X="XCFLAGS=-DLUAJIT_DISABLE_JIT"; else X="XCFLAGS="; fi
  echo "-- build real-$v (runtime/*.lua, libnlc)"
  if ! "$ROOT/tests/vm/build-vm.sh" "$T/real-$v" "NL_LIBNLC=$LIBNLC" \
       "NL_LIBNLC_LIBS=$NATIVE" "$X" >"$T/real-$v.log" 2>&1; then
    tail -20 "$T/real-$v.log"
    echo "e2e: build real-$v failed"
    STATUS=1
    continue
  fi
  B=$T/real-$v/src
  # shellcheck disable=SC2086
  if ! $CC -g -Wall -I"$B" -o "$T/e2e-host-$v" "$ROOT/tests/vm/e2e/host.c" \
       "$B/libluajit.a" "$LIBNLC" $NATIVE -lm; then
    echo "e2e: cannot link the host against libluajit.a and libnlc"
    STATUS=1
    continue
  fi
  echo "-- host ($v)"
  (cd "$P" && "$T/e2e-host-$v") || STATUS=1
  echo "-- luajit main.lua ($v)"
  if (cd "$P" && "$B/luajit" main.lua) >"$T/e2e-main-$v.out" 2>"$T/e2e-main-$v.err" &&
     cmp -s "$T/e2e-main-$v.out" "$P/main.out"; then
    echo "ok - main.lua prints main.out"
  else
    echo "not ok - main.lua prints main.out; got:"
    cat "$T/e2e-main-$v.out" "$T/e2e-main-$v.err"
    STATUS=1
  fi
done
[ $STATUS = 0 ] && echo "e2e: passed" || echo "e2e: FAILED"
exit $STATUS
