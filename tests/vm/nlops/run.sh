#!/bin/sh
# The NL opcodes of the fork: test_nlops.lua
# assembles bytecode dumps by hand (bcasm.lua) and checks KINTR, ISMETH,
# MSELF with CALLM/CALLMT, CMPGT/CMPNGT/CMPGE/CMPNGE, __len on tables and
# the dump loader's flag checks on the interpreter. It runs them on the
# real-runtime builds of tests/vm/run.sh: the JIT build plainly, with -joff
# and with -Ohotloop=1 (traces cross the opcodes; the tests that warm a
# function up check the recorder's guards), and the LUAJIT_DISABLE_JIT
# build.
#   tests/vm/nlops/run.sh
# The builds are target/vm/real-jit and target/vm/real-nojit, as
# tests/vm/text/run.sh makes them (NL_CONFORM_JIT=off for the latter). This
# script does not build; a binary older than LuaJIT/src is stale, and its
# vmdef check will say so.
set -u
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
T=$ROOT/target/vm
TEST=$ROOT/tests/vm/nlops/test_nlops.lua
fail=0

run() {  # NAME LUAJIT [options...]
  name=$1; shift
  exe=$1
  if [ ! -x "$exe" ]; then
    echo "FAIL $name: no interpreter at $exe (build it with tests/vm/text/run.sh)"
    fail=$((fail + 1))
    return
  fi
  out=$("$@" "$TEST" 2>&1)
  st=$?
  if [ $st = 0 ]; then
    echo "ok   $name: $(printf '%s\n' "$out" | tail -1)"
  else
    fail=$((fail + 1))
    echo "FAIL $name (exit $st)"
    printf '%s\n' "$out" | grep -v '^ok - ' | head -20 | sed 's/^/    /'
  fi
}

run "real-jit" "$T/real-jit/src/luajit"
run "real-jit -joff" "$T/real-jit/src/luajit" -joff
run "real-jit -Ohotloop=1" "$T/real-jit/src/luajit" -Ohotloop=1
run "real-nojit" "$T/real-nojit/src/luajit"

echo "nlops tests: $((4 - fail)) of 4 runs passed"
[ $fail = 0 ]
