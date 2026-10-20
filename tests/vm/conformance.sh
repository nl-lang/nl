#!/bin/sh
# The conformance programs of tests/run on the fork, with the rules of
# tests/run/README.md: working directory tests/run, NL_PATH unset, stdout
# compared byte for byte with the .out file, exit status 0, a time limit,
# the declaration files of tests/run/decl in scope (nlrun --decl decl);
# after the main chunk, pump until no task is pending (as `nlc run` does).
# Every program runs, the worker_* ones included. The worker_* programs run
# three times, with 0, 1 and 8 pool workers (NL_CONFORM_WORKERS, which nlrun
# applies with nl_setworkers); with NL_CONFORM_WORKER_MODES (for example "0" or "2 4"), those numbers. The rejected programs must fail
# to load with the error on the line after their "# expect-error:" comment.
#
# Each program runs in a fresh process of tests/vm/conform/nlrun.c, a C host
# linked with the real compiler and the real runtime (JIT on; with
# NL_CONFORM_JIT=off, the JIT-less build; with NL_CONFORM_JIT=hot, the JIT
# forced with hotloop=1). The compiler emits bytecode here (nlc.h), so this
# is the bytecode backend's run on the fork. With NL_CONFORM_SANDBOX=1 each
# program runs under the sandbox profile (nlrun --sandbox, NL_OPT_SANDBOX);
# tests/vm/real/run.sh does that for a list of programs. Results:
# target/vm/conform/.
#   tests/vm/conformance.sh [program.nl ...]
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
T=$ROOT/target/vm
R=$T/conform
CC=${CC:-cc}
case "$(uname -s)" in
  Darwin) : "${MACOSX_DEPLOYMENT_TARGET:=11.0}"; export MACOSX_DEPLOYMENT_TARGET; NATIVE="" ;;
  Linux)  NATIVE="-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc" ;;
  *)      NATIVE="" ;;
esac
V=jit; X="XCFLAGS="
[ "${NL_CONFORM_JIT:-on}" = off ] && { V=nojit; X="XCFLAGS=-DLUAJIT_DISABLE_JIT"; }
# NL_CONFORM_BUILD names the build directory under target/vm (default
# real-$V), so that a build in progress elsewhere is left alone.
D=${NL_CONFORM_BUILD:-real-$V}
mkdir -p "$R"
cargo build -q -p nl_capi --release --manifest-path "$ROOT/Cargo.toml" || { echo "cannot build libnlc"; exit 1; }
LIBNLC=$ROOT/target/release/libnl_capi.a
"$ROOT/tests/vm/build-vm.sh" "$T/$D" "NL_LIBNLC=$LIBNLC" "NL_LIBNLC_LIBS=$NATIVE" "$X" \
  >"$T/$D.log" 2>&1 || { tail -20 "$T/$D.log"; echo "build failed"; exit 1; }
B=$T/$D/src
# shellcheck disable=SC2086
$CC -g -O1 -Wall -I"$B" -o "$R/nlrun-$D" "$ROOT/tests/vm/conform/nlrun.c" "$B/libluajit.a" \
  "$LIBNLC" $NATIVE -lm || { echo "cannot build nlrun"; exit 1; }

run_with() {  # TIMEOUT CMD...
  t=$1; shift
  perl -e '$t = shift; $SIG{ALRM} = sub { kill 9, $pid; exit 124 };
           $pid = fork; if ($pid == 0) { exec @ARGV; exit 127 }
           alarm $t; waitpid $pid, 0; exit($? & 127 ? 128 + ($? & 127) : $? >> 8)' "$t" "$@"
}

SBX=""; SFX=""
[ "${NL_CONFORM_SANDBOX:-0}" = 1 ] && { SBX="--sandbox"; SFX=".sandbox"; }

cd "$ROOT/tests/run" || exit 1
unset NL_PATH
if [ $# -gt 0 ]; then LIST="$*"; else LIST=$(ls *.nl); fi
pass=0; fail=0; skip=0; FAILS=""
for f in $LIST; do
  base=${f%.nl}
  [ -f "$base.out" ] || continue           # helpers and rejects
  case "$base" in
    worker_*) MODES=${NL_CONFORM_WORKER_MODES:-0 1 8} ;;
    *) MODES="-" ;;
  esac
  for wmode in $MODES; do
    WSFX=""
    unset NL_CONFORM_WORKERS
    [ "$wmode" != - ] && { WSFX=".w$wmode"; NL_CONFORM_WORKERS=$wmode; export NL_CONFORM_WORKERS; }
    # shellcheck disable=SC2086
    run_with 30 "$R/nlrun-$D" $SBX --decl decl "$f" >"$R/$base$SFX$WSFX.stdout" 2>"$R/$base$SFX$WSFX.stderr"
    st=$?
    if [ $st = 0 ] && cmp -s "$R/$base$SFX$WSFX.stdout" "$base.out"; then
      pass=$((pass + 1))
    else
      fail=$((fail + 1)); FAILS="$FAILS $base$WSFX"
      echo "FAIL $base${WSFX:+ ($wmode workers)} (exit $st)"
      diff "$base.out" "$R/$base$SFX$WSFX.stdout" | head -8 | sed 's/^/    /'
      head -5 "$R/$base$SFX$WSFX.stderr" | sed 's/^/    stderr: /'
    fi
  done
done
# Rejected programs: loading must fail, and the error must point at the line
# after the "# expect-error:" comment (README, "Rejected programs").
if [ $# = 0 ]; then
  for f in reject/*.nl; do
    base=${f%.nl}; name=${base#reject/}
    line=$(grep -n 'expect-error:' "$f" | head -1 | cut -d: -f1)
    line=$((line + 1))
    run_with 30 "$R/nlrun-$D" --decl decl "$f" >"$R/reject_$name.stdout" 2>"$R/reject_$name.stderr"
    st=$?
    if [ $st != 0 ] && grep -q " $f:$line:" "$R/reject_$name.stderr"; then
      pass=$((pass + 1))
    else
      fail=$((fail + 1)); FAILS="$FAILS $base"
      echo "FAIL $base (exit $st, expected an error at line $line)"
      head -3 "$R/reject_$name.stderr" | sed 's/^/    stderr: /'
    fi
  done
fi
unset NL_CONFORM_WORKERS
echo "conformance on the fork (JIT ${NL_CONFORM_JIT:-on}${SBX:+, sandbox}): $pass passed, $fail failed, $skip excluded"
[ $fail = 0 ]
