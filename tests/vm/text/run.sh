#!/bin/sh
# The regex and string libraries of the fork: NL
# programs under tests/vm/text, run as the conformance programs are
# (tests/vm/conformance.sh: a fresh process of tests/vm/conform/nlrun.c, the
# real compiler and runtime, stdout compared with the .out file, exit status
# 0, a time limit). They test what the conformance programs of tests/run
# leave out: linear matching time on a pathological pattern, the memory
# charged to the collector, the pattern cache, empty matches, many groups,
# the library seen from Lua, and the string iterator under the JIT.
#   tests/vm/text/run.sh [program.nl ...]      (NL_CONFORM_JIT=off: JIT off;
#                                               NL_TEXT_ASSERT=1: a build with
#                                               LuaJIT's assertions, which check
#                                               at lua_close that the memory
#                                               charged for regexes was taken
#                                               off again)
set -u
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
T=$ROOT/target/vm
R=$T/text
CC=${CC:-cc}
case "$(uname -s)" in
  Darwin) : "${MACOSX_DEPLOYMENT_TARGET:=11.0}"; export MACOSX_DEPLOYMENT_TARGET; NATIVE="" ;;
  Linux)  NATIVE="-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc" ;;
  *)      NATIVE="" ;;
esac
V=jit; X="XCFLAGS="; D="CCDEBUG="
[ "${NL_CONFORM_JIT:-on}" = off ] && { V=nojit; X="XCFLAGS=-DLUAJIT_DISABLE_JIT"; }
[ "${NL_TEXT_ASSERT:-0}" = 1 ] && { V=assert; X="XCFLAGS=-DLUA_USE_ASSERT -DLUA_USE_APICHECK"; D="CCDEBUG=-g"; }
mkdir -p "$R"
cargo build -q -p nl_capi --release --manifest-path "$ROOT/Cargo.toml" || { echo "cannot build libnlc"; exit 1; }
LIBNLC=$ROOT/target/release/libnl_capi.a
"$ROOT/tests/vm/build-vm.sh" "$T/real-$V" "NL_LIBNLC=$LIBNLC" "NL_LIBNLC_LIBS=$NATIVE" "$X" "$D" \
  >"$T/real-$V.log" 2>&1 || { tail -20 "$T/real-$V.log"; echo "build failed"; exit 1; }
B=$T/real-$V/src
# shellcheck disable=SC2086
$CC -g -O1 -Wall -I"$B" -o "$R/nlrun-$V" "$ROOT/tests/vm/conform/nlrun.c" "$B/libluajit.a" \
  "$LIBNLC" $NATIVE -lm || { echo "cannot build nlrun"; exit 1; }

run_with() {  # TIMEOUT CMD...
  t=$1; shift
  perl -e '$t = shift; $SIG{ALRM} = sub { kill 9, $pid; exit 124 };
           $pid = fork; if ($pid == 0) { exec @ARGV; exit 127 }
           alarm $t; waitpid $pid, 0; exit($? & 127 ? 128 + ($? & 127) : $? >> 8)' "$t" "$@"
}

cd "$ROOT/tests/vm/text" || exit 1
unset NL_PATH
if [ $# -gt 0 ]; then LIST="$*"; else LIST=$(ls *.nl); fi
pass=0; fail=0
for f in $LIST; do
  base=${f%.nl}
  [ -f "$base.out" ] || continue
  run_with 60 "$R/nlrun-$V" "$f" >"$R/$base.stdout" 2>"$R/$base.stderr"
  st=$?
  if [ $st = 0 ] && cmp -s "$R/$base.stdout" "$base.out"; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    echo "FAIL $base (exit $st)"
    diff "$base.out" "$R/$base.stdout" | head -8 | sed 's/^/    /'
    head -5 "$R/$base.stderr" | sed 's/^/    stderr: /'
  fi
done
echo "text tests on the fork ($V): $pass passed, $fail failed"
[ $fail = 0 ]
