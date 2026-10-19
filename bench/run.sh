#!/bin/sh
# NL against hand-written Lua on the fork.
#
#   bench/run.sh [pair ...]
#
# Builds the compiler library, the fork with the real compiler and runtime
# twice (target/vm/bench-jit, and target/vm/bench-nojit with
# -DLUAJIT_DISABLE_JIT, the interpreter-only build) and the host
# bench/nlbench.c against each. Then, for every pair bench/NAME.nl and
# bench/NAME.lua (all of them, or those named):
#
# - checks that the Lua program, the NL program and the NL program compiled
#   with --unchecked (NL_OPT_CHECKS=0) print the same checksum, in both
#   builds;
# - times the three, NLB_RUNS rounds each (default 5), interleaved, as the
#   wall-clock time of the whole process (state creation and the NL compile
#   included: see the `empty` pair), and keeps the minimum and the median;
# - measures a pair again (up to NLB_RETRIES more times, default 2) when the
#   median of any of the three is more than 10% above its minimum.
#
# Before timing it waits until no cargo, rustc, cc, clang or make process
# has been seen for a minute (checks every 30 s), and checks again before
# each pair. NLB_NOWAIT=1 skips the wait, for trying things out.
#
# Every program reads NLB_SCALE (default 1), the number of times it repeats
# its pinned workload. The interpreter runs at scale 1; the JIT build runs
# at the scale given below, so that the JIT runs are long enough to time.
# The table goes to stdout and to bench/RESULTS.md.
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
T=$ROOT/target/vm
BENCH=$ROOT/bench
RUNS=${NLB_RUNS:-5}
RETRIES=${NLB_RETRIES:-2}
CC=${CC:-cc}
case "$(uname -s)" in
  Darwin) : "${MACOSX_DEPLOYMENT_TARGET:=11.0}"; export MACOSX_DEPLOYMENT_TARGET; NATIVE="" ;;
  Linux)  NATIVE="-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc" ;;
  *)      NATIVE="" ;;
esac

# The scale of each pair in the JIT build.
jit_scale() {
  case $1 in
    num_float|num_int|case_match) echo 8 ;;
    fields|fields_typed|methods|methods_typed) echo 10 ;;
    interp) echo 6 ;;
    raytrace|try_ok|bitops|bitops_typed|struct_import) echo 4 ;;
    floordiv) echo 6 ;;
    lua_path) echo 8 ;;
    list_index) echo 2 ;;
    *) echo 1 ;;
  esac
}

# -- Build ------------------------------------------------------------------

mkdir -p "$T"
echo "building libnlc" >&2
cargo build -q -p nl_capi --release --manifest-path "$ROOT/Cargo.toml" || { echo "cannot build libnlc" >&2; exit 1; }
LIBNLC=$ROOT/target/release/libnl_capi.a
for v in jit nojit; do
  X="XCFLAGS="
  [ $v = nojit ] && X="XCFLAGS=-DLUAJIT_DISABLE_JIT"
  echo "building the fork: target/vm/bench-$v" >&2
  "$ROOT/tests/vm/build-vm.sh" "$T/bench-$v" "NL_LIBNLC=$LIBNLC" "NL_LIBNLC_LIBS=$NATIVE" "$X" \
    >"$T/bench-$v.log" 2>&1 || { tail -20 "$T/bench-$v.log" >&2; echo "build failed" >&2; exit 1; }
  B=$T/bench-$v/src
  # shellcheck disable=SC2086
  $CC -O2 -Wall -I"$B" -o "$T/nlbench-$v" "$BENCH/nlbench.c" "$B/libluajit.a" "$LIBNLC" $NATIVE -lm \
    || { echo "cannot build nlbench" >&2; exit 1; }
done

# -- Pairs ------------------------------------------------------------------

if [ $# -gt 0 ]; then
  PAIRS="$*"
else
  PAIRS=$(cd "$BENCH" && for f in *.nl; do n=${f%.nl}; [ -f "$n.lua" ] && echo "$n"; done)
fi
# `empty` comes first: its times are the startup cost that the net ratios
# subtract.
PAIRS="empty $(for p in $PAIRS; do [ "$p" = empty ] || echo "$p"; done)"
for p in $PAIRS; do
  [ -f "$BENCH/$p.nl" ] && [ -f "$BENCH/$p.lua" ] || { echo "no pair named $p" >&2; exit 2; }
done

# -- Quiet machine ----------------------------------------------------------

BUSY='cargo|rustc|cc|clang|make'
WAITED=0
wait_quiet() {
  [ "${NLB_NOWAIT:-0}" = 1 ] && return 0
  quiet=0
  while [ $quiet -lt 3 ]; do
    if pgrep -x "$BUSY" >/dev/null 2>&1; then
      echo "waiting for: $(pgrep -lx "$BUSY" | awk '{print $2}' | sort -u | tr '\n' ' ')" >&2
      quiet=0
      WAITED=$((WAITED + 30))
    else
      quiet=$((quiet + 1))
      [ $quiet -ge 3 ] && break
    fi
    sleep 30
  done
}

load() { uptime | sed 's/.*load averages*: *//'; }

# -- Timing -----------------------------------------------------------------

# measure MODE PAIR: prints "lua_min lua_med nl_min nl_med un_min un_med".
measure() {
  perl -MTime::HiRes=time -e '
    my ($runs, $exe, $p) = @ARGV;
    my @cmds = ([$exe, "$p.lua"], [$exe, "$p.nl"], [$exe, "--unchecked", "$p.nl"]);
    my @t = ([], [], []);
    for my $r (0 .. $runs - 1) {
      for my $k (0 .. 2) {
        my $i = ($k + $r) % 3;    # rotate the order from round to round
        my $t0 = time;
        my $pid = fork;
        if ($pid == 0) { open STDOUT, ">", "/dev/null"; exec @{$cmds[$i]}; exit 127 }
        waitpid $pid, 0;
        die "@{$cmds[$i]} failed\n" if $?;
        push @{$t[$i]}, time - $t0;
      }
    }
    my @out;
    for my $i (0 .. 2) {
      my @s = sort { $a <=> $b } @{$t[$i]};
      my $n = @s;
      my $med = $n % 2 ? $s[($n - 1) / 2] : ($s[$n / 2 - 1] + $s[$n / 2]) / 2;
      push @out, sprintf("%.4f %.4f", $s[0], $med);
    }
    print join(" ", @out), "\n";
  ' "$RUNS" "$T/nlbench-$1" "$BENCH/$2"
}

# spread "min med ...": the largest (med - min) / min of the three, in %.
spread() {
  echo "$1" | awk '{ m = 0; for (i = 1; i < NF; i += 2) { s = ($(i + 1) - $i) / $i * 100; if (s > m) m = s }
    printf "%.1f\n", m }'
}

# fmt_row PAIR MODE SCALE "six numbers" ATTEMPTS "startup times": one row.
# The net ratios subtract the minima of the `empty` pair (same build, same
# side) from both times, which matters for the short JIT runs.
fmt_row() {
  echo "$4 $6" | awk -v p="$1" -v m="$2" -v sc="$3" -v at="$5" '
    function net(x, ex, y, ey) { return (y - ey > 0.0005) ? sprintf("%.2f", (x - ex) / (y - ey)) : "-" }
    {
      note = (at > 1) ? " (measured " at " times)" : ""
      printf "| %s | %s%s | %s | %.3f / %.3f | %.3f / %.3f | **%.2f** | %s | %.3f / %.3f | %.2f | %s |\n",
        p, m, note, sc, $1, $2, $3, $4, $3 / $1, (p == "empty") ? "-" : net($3, $9, $1, $7),
        $5, $6, $5 / $1, (p == "empty") ? "-" : net($5, $11, $1, $7)
    }'
}

OUT=$BENCH/RESULTS.md.tmp
LOAD_BEFORE=$(load)
echo "checking that the machine is quiet" >&2
wait_quiet
START=$(date '+%Y-%m-%d %H:%M')
ROWS=""
MISMATCH=""
NOISY=""
for p in $PAIRS; do
  for mode in nojit jit; do
    if [ $mode = jit ]; then scale=$(jit_scale "$p"); else scale=1; fi
    export NLB_SCALE=$scale
    # The checksums must agree.
    exe=$T/nlbench-$mode
    lo=$("$exe" "$BENCH/$p.lua" 2>&1)
    no=$("$exe" "$BENCH/$p.nl" 2>&1)
    uo=$("$exe" --unchecked "$BENCH/$p.nl" 2>&1)
    if [ "$lo" != "$no" ] || [ "$lo" != "$uo" ]; then
      echo "MISMATCH $p ($mode): lua [$lo] nl [$no] unchecked [$uo]" >&2
      MISMATCH="$MISMATCH $p/$mode"
      continue
    fi
    wait_quiet
    best=""; bests=1000000; att=0
    while [ $att -le "$RETRIES" ]; do
      att=$((att + 1))
      r=$(measure $mode "$p") || { echo "$p failed" >&2; exit 1; }
      s=$(spread "$r")
      if awk -v s="$s" -v b="$bests" 'BEGIN { exit !(s < b) }'; then best=$r; bests=$s; fi
      awk -v s="$s" 'BEGIN { exit !(s <= 10) }' && break
      echo "$p ($mode): median ${s}% above minimum, again" >&2
    done
    awk -v s="$bests" 'BEGIN { exit !(s > 10) }' && NOISY="$NOISY $p/$mode(${bests}%)"
    [ "$p" = empty ] && eval "EMPTY_$mode=\"\$best\""
    eval "startup=\"\${EMPTY_$mode:-0 0 0 0 0 0}\""
    row=$(fmt_row "$p" "$([ $mode = jit ] && echo JIT || echo interp)" "$scale" "$best" "$att" "$startup")
    echo "$row" >&2
    ROWS="$ROWS$row
"
  done
done
LOAD_AFTER=$(load)

MACHINE="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m), $( (sysctl -n hw.ncpu 2>/dev/null || nproc) | head -1) cores, $(uname -s) $(sw_vers -productVersion 2>/dev/null || uname -r)"
{
  echo "# Benchmark results"
  echo
  echo "Machine: $MACHINE. Started $START; commit $(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null)," \
    "fork $(git -C "$ROOT/LuaJIT" rev-parse --short HEAD 2>/dev/null)."
  echo
  echo "Load averages (1, 5, 15 min): $LOAD_BEFORE before, $LOAD_AFTER after. Waited ${WAITED} s for" \
    "cargo/rustc/cc/clang/make to finish before timing."
  echo
  echo "Each cell is the wall-clock time of the whole process in seconds, minimum / median of $RUNS runs" \
    "(\`bench/run.sh\`). *interp* is the build with \`-DLUAJIT_DISABLE_JIT\`, *JIT* the default build," \
    "both the fork with the real compiler and runtime, hosted by \`bench/nlbench.c\`. *scale* is how many" \
    "times the program repeats its workload (NLB_SCALE). The ratios divide minima; *net* ratios first" \
    "subtract the minimum of the \`empty\` pair (startup: state, runtime, compile) of the same build and side." \
    "*NL unchecked* is the" \
    "NL program compiled with NL_OPT_CHECKS=0 (\`nlc build --unchecked\`)."
  echo
  echo "| pair | mode | scale | Lua (min / med) | NL (min / med) | NL/Lua | net | NL unchecked (min / med) | unchecked/Lua | net |"
  echo "|---|---|---|---|---|---|---|---|---|---|"
  printf '%s' "$ROWS"
  echo
  if [ -n "$MISMATCH" ]; then echo "Checksums differ, not timed:$MISMATCH."; else echo "All checksums agree."; fi
  [ -n "$NOISY" ] && echo "Median still more than 10% above the minimum after $((RETRIES + 1)) runs:$NOISY."
} >"$OUT"
if [ $# -eq 0 ]; then mv "$OUT" "$BENCH/RESULTS.md"; else mv "$OUT" "$T/bench-results.md"; fi
cat "$([ $# -eq 0 ] && echo "$BENCH/RESULTS.md" || echo "$T/bench-results.md")"
[ -z "$MISMATCH" ]
