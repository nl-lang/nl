#!/bin/sh
# Baseline compatibility of the VM fork:
# Lua code behaves identically on the fork and on stock LuaJIT, with the JIT
# on and off.
#
#   tests/lua-compat/run.sh
#
# 1. fetch.sh and build.sh (stock and fork interpreters, JIT on and off).
# 2. The upstream suite (LuaJIT-test-cleanup, test/test.lua +slow) on every
#    interpreter, in four modes: the JIT build as it is, with -joff, with
#    -Ohotloop=1, and the JIT-less build. Test by test (by position), fork
#    and stock must print the same, except for the differences listed in
#    expected-differences.txt, each checked exactly (compare.lua).
# 3. A bytecode corpus: every .lua file of the upstream test suite and
#    benchmarks, compiled by the stock luajit with -b (stripped) and with -bg
#    (debug info), then run on stock and fork in the same four modes. Output
#    (stdout, stderr, exit status; addresses and the interpreter's path
#    normalized) must be identical, or differ exactly as an entry of
#    expected-differences.txt says. The benchmarks that print their own
#    timings, and files whose output differs between two runs on stock
#    itself, are compared by exit status only.
#
# Everything runs twice: with the fork built with the fake runtime
# (tests/vm/fake_runtime: the VM changes alone) and with the real one
# (runtime/*.lua: what ships). NL_COMPAT_RUNTIME=fake or real runs one.
#
# Results: target/lua-compat/results/ (fake), results-real/ (real). Exit
# status 0 when nothing differs but what is listed.
set -u
if [ -z "${NL_COMPAT_RUNTIME:-}" ]; then
  st=0
  for rt in fake real; do
    echo "######## fork built with the $rt runtime"
    NL_COMPAT_RUNTIME=$rt "$0" "$@" || st=1
  done
  exit $st
fi
# Outputs are compared as bytes: some tests print bytes that are not UTF-8.
LC_ALL=C
export LC_ALL
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
T=$ROOT/target/lua-compat
S=$T/LuaJIT-test-cleanup
case "${NL_COMPAT_RUNTIME:-fake}" in
  real) R=$T/results-real; FORK=fork-real ;;
  *) R=$T/results; FORK=fork ;;
esac
# The build directory of a side (stock or fork) and a build (jit or nojit).
bdir() { if [ "$1" = fork ]; then echo "$T/$FORK-$2"; else echo "$T/$1-$2"; fi; }
"$ROOT/tests/lua-compat/fetch.sh" || exit 1
"$ROOT/tests/lua-compat/build.sh" || exit 1
mkdir -p "$R"
DIFFS=0

# run_with TIMEOUT CMD... : runs with a time limit (perl, for macOS).
run_with() {
  t=$1; shift
  perl -e '$t = shift; $SIG{ALRM} = sub { kill 9, $pid; exit 124 };
           $pid = fork; if ($pid == 0) { exec @ARGV; exit 127 }
           alarm $t; waitpid $pid, 0; exit($? & 127 ? 128 + ($? & 127) : $? >> 8)' "$t" "$@"
}

normalize() {  # normalize BIN < in > out
  sed -e "s#$1#luajit#g" -e "s#$(dirname "$1")#BUILDDIR#g" \
      -e 's/0x[0-9a-fA-F]\{4,\}/0xADDR/g'
}

EXP=$ROOT/tests/lua-compat/expected-differences.txt
COMPARE="$T/stock-jit/src/luajit $ROOT/tests/lua-compat/compare.lua"

# Modes: name, build, options.
MODES="jit:jit: joff:jit:-joff hot:jit:-Ohotloop=1 nojit:nojit:"

# -- 2. Upstream suite -------------------------------------------------------
echo "== upstream suite (test/test.lua +slow)"
for m in $MODES; do
  name=${m%%:*}; rest=${m#*:}; build=${rest%%:*}; opt=${rest#*:}
  for side in stock fork; do
    bin=$(bdir "$side" "$build")/src/luajit
    # shellcheck disable=SC2086
    (cd "$S/test" && run_with 600 "$bin" $opt test.lua +slow) 2>&1 |
      normalize "$bin" >"$R/suite-$side-$name.out"
  done
  sum_s=$(grep -E '^[0-9]+ passed' "$R/suite-stock-$name.out" | tail -1)
  sum_f=$(grep -E '^[0-9]+ passed' "$R/suite-fork-$name.out" | tail -1)
  echo "  $name: stock: $sum_s; fork: $sum_f"
  # shellcheck disable=SC2086
  if ! $COMPARE suite "$R/suite-stock-$name.out" "$R/suite-fork-$name.out" \
       "$EXP" "$NL_COMPAT_RUNTIME"; then
    echo "  $name: UNEXPECTED DIFFERENCES"
    DIFFS=$((DIFFS + 1))
  fi
done

# -- 3. Bytecode corpus ------------------------------------------------------
echo "== bytecode corpus (compiled by stock luajit -b and -bg)"
C=$T/corpus
mkdir -p "$C"
STOCK=$T/stock-jit/src/luajit
LUA_PATH="$T/stock-jit/src/?.lua;;"   # jit.bcsave for -b
export LUA_PATH
# Benchmarks take a size argument; small ones keep the run short. Those that
# read stdin get a file. scimark prints timings, so it is compared by status.
bench_args() {
  case "$1" in
    array3d) echo 30 ;; binary-trees) echo 8 ;; chameneos) echo 10000 ;;
    coroutine-ring) echo 100000 ;; euler14-bit) echo 100000 ;; fannkuch) echo 7 ;;
    fasta) echo 1000 ;; mandelbrot|mandelbrot-bit) echo 100 ;; md5) echo 10 ;;
    nbody) echo 1000 ;; nsieve|nsieve-bit|nsieve-bit-fp) echo 4 ;;
    partialsums) echo 10000 ;; pidigits-nogmp) echo 100 ;; ray) echo 2 ;;
    recursive-ack) echo 5 ;; recursive-fib) echo 20 ;; scimark-fft) echo 100 ;;
    scimark-lu) echo 50 ;; scimark-sor) echo 100 ;; scimark-sparse) echo 1000 ;;
    series) echo 100 ;; spectral-norm) echo 50 ;; *) echo "" ;;
  esac
}
"$STOCK" "$S/bench/fasta.lua" 1000 >"$C/fasta1000.txt"
# Benchmarks that print how long they took: compared by exit status only.
timed() {
  case "$1" in bench/scimark-2010-12-20.lua|bench/series.lua) return 0 ;; *) return 1 ;; esac
}
bench_stdin() {
  case "$1" in
    k-nucleotide|revcomp) echo "$C/fasta1000.txt" ;;
    sum-file) echo "$S/bench/SUMCOL_1.txt" ;;
    *) echo /dev/null ;;
  esac
}

: >"$C/list"
nfiles=0; nbad=0
for f in $(cd "$S" && find test bench -name '*.lua' | sort); do
  key=$(printf '%s' "$f" | tr '/' '_' | sed 's/\.lua$//')
  ok=1
  "$STOCK" -b "$S/$f" "$C/$key.s.bc" 2>/dev/null || ok=0
  "$STOCK" -bg "$S/$f" "$C/$key.g.bc" 2>/dev/null || ok=0
  if [ $ok = 1 ]; then
    printf '%s %s\n' "$f" "$key" >>"$C/list"
    nfiles=$((nfiles + 1))
  else
    nbad=$((nbad + 1))
    echo "  not compiled by stock luajit -b: $f"
  fi
done
echo "  $nfiles files compiled by stock luajit -b and -bg ($nbad do not compile on stock and are left out)"

# run_one SIDE BUILD OPT FILE KEY FORM -> prints the normalized result
run_one() {
  bin=$(bdir "$1" "$2")/src/luajit
  case "$4" in
    bench/*) dir=$S/bench; b=$(basename "$4" .lua); args=$(bench_args "$b"); in=$(bench_stdin "$b") ;;
    *) dir=$S/test; args=""; in=/dev/null ;;
  esac
  # shellcheck disable=SC2086
  (cd "$dir" && LUA_PATH="$dir/?.lua;$S/test/?.lua;$(bdir "$1" "$2")/src/?.lua;;" run_with 60 "$bin" $3 "$C/$5.$6.bc" $args <"$in") \
    >"$R/one.out" 2>&1
  st=$?
  normalize "$bin" <"$R/one.out"
  echo "exit status $st"
}

total=0; same=0; statusonly=0; differ=0; expected=0
: >"$R/corpus-expected.txt"
: >"$R/corpus-status-only.txt"
: >"$R/corpus-differences.txt"
for m in $MODES; do
  name=${m%%:*}; rest=${m#*:}; build=${rest%%:*}; opt=${rest#*:}
  mt=0; ms=0; md=0; mo=0; me=0
  while read -r f key; do
    for form in s g; do
      run_one stock "$build" "$opt" "$f" "$key" "$form" >"$R/a.out"
      run_one fork "$build" "$opt" "$f" "$key" "$form" >"$R/b.out"
      mt=$((mt + 1))
      if cmp -s "$R/a.out" "$R/b.out"; then
        ms=$((ms + 1))
      else
        # Nondeterministic on stock itself? Then compare exit status only.
        run_one stock "$build" "$opt" "$f" "$key" "$form" >"$R/a2.out"
        samest=0; [ "$(tail -1 "$R/a.out")" = "$(tail -1 "$R/b.out")" ] && samest=1
        # shellcheck disable=SC2086
        if timed "$f" && [ $samest = 1 ]; then
          mo=$((mo + 1))
          echo "$name $f ($form)" >>"$R/corpus-status-only.txt"
        elif why=$($COMPARE corpus "$f" "$R/a.out" "$R/b.out" "$EXP" "$NL_COMPAT_RUNTIME" 2>&1); then
          me=$((me + 1))
          echo "$name $f ($form): $why" >>"$R/corpus-expected.txt"
        elif ! cmp -s "$R/a.out" "$R/a2.out" && [ $samest = 1 ]; then
          mo=$((mo + 1))
          echo "$name $f ($form)" >>"$R/corpus-status-only.txt"
        else
          md=$((md + 1))
          { echo "### $name $f ($form): $why"; diff "$R/a.out" "$R/b.out" | head -10; } >>"$R/corpus-differences.txt"
        fi
      fi
    done
  done <"$C/list"
  echo "  $name: $mt runs: $ms identical, $me expected differences, $mo same exit status (output varies run to run on stock too), $md different"
  total=$((total + mt)); same=$((same + ms)); statusonly=$((statusonly + mo)); differ=$((differ + md)); expected=$((expected + me))
done
echo "  corpus total: $total runs, $same identical, $expected expected differences, $statusonly status-only, $differ different"
echo "  expected differences: $R/corpus-expected.txt"
echo "  status-only: $(sed 's/ (.)$//' "$R/corpus-status-only.txt" | awk '{print $2}' | sort -u | tr '\n' ' ')"
if [ $differ -gt 0 ]; then
  echo "  differences: $R/corpus-differences.txt"
  head -40 "$R/corpus-differences.txt"
  DIFFS=$((DIFFS + 1))
fi

if [ $DIFFS = 0 ]; then
  echo "lua-compat: fork and stock agree"
else
  echo "lua-compat: DIFFERENCES FOUND"
  exit 1
fi
