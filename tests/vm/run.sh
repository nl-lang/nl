#!/bin/sh
# The VM tests (tests/vm): one command.
#
#   tests/vm/run.sh            all of the below
#   tests/vm/run.sh --no-tsan  without the ThreadSanitizer build
#   tests/vm/run.sh --no-asan  without the AddressSanitizer build
#
# On arm64 Linux the sanitizer builds are skipped (see ASAN=0 below).
#
# 1. Builds the fork three times, JIT on, JIT off (LUAJIT_DISABLE_JIT), and
#    JIT on with LuaJIT's assertions and API checks (LUA_USE_ASSERT,
#    LUA_USE_APICHECK), with the fake runtime (tests/vm/fake_runtime) and the
#    identity compiler (tests/vm/support/nlc_identity.c), under
#    target/vm/fake-{jit,nojit,assert}.
# 2. For each: test_capi (C API), test_events.lua (event library; for the JIT
#    build also with -joff and with -Ohotloop=1), test_threads.
# 3. An AddressSanitizer + UndefinedBehaviorSanitizer build (JIT off, the
#    system allocator, assertions and API checks) and test_capi,
#    test_events.lua and test_threads under it. Any report fails.
# 4. A ThreadSanitizer build (JIT off) and test_threads under it.
# 5. With the real compiler (crates/nl_capi) and the real runtime
#    (runtime/*.lua): test_capi_real (the C API against the real library),
#    test_sandbox (the sandbox profile, with some conformance programs run
#    under it), test_transfer (the transfer encoder) and test_workers (the
#    workers), all also with assertions and API checks
#    (real-assert); test_transfer and test_workers with ASan and UBSan
#    (real-asan); test_workers' stress test and test_threads under
#    ThreadSanitizer with the real library (real-tsan; the Rust library
#    itself is not instrumented); tests/vm/e2e/run.sh (the end-to-end test),
#    tests/vm/conformance.sh (the conformance programs of tests/run on the
#    fork, the worker programs with 0, 1 and 8 pool workers) and
#    tests/vm/text/run.sh (the regex and string libraries), JIT on
#    and off.
# 6. The Rust side, each crate built outside the
#    workspace against the fork through [patch.crates-io] luajit-src:
#    tests/vm/nl_sys_link (nl_sys links; without the patch it must not),
#    tests/vm/nl_host (the nl crate, and a C embedding example beside its
#    Rust twin) and tests/vm/nl_example (an example host built as a fresh
#    crate).
#
# Everything is built under target/vm. Build logs: target/vm/*.log.
set -u

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
T=$ROOT/target/vm
TSAN=1
ASAN=1
for a in "$@"; do
  case "$a" in
    --no-tsan) TSAN=0 ;;
    --no-asan) ASAN=0 ;;
    *) echo "unknown option $a" >&2; exit 2 ;;
  esac
done
mkdir -p "$T/tmp"
case "$(uname -s)" in
  Darwin) : "${MACOSX_DEPLOYMENT_TARGET:=11.0}"; export MACOSX_DEPLOYMENT_TARGET ;;
esac
CC=${CC:-cc}
LIBS="-lm"
case "$(uname -s)" in Linux) LIBS="-lm -ldl -lpthread" ;; esac

# The ASan, UBSan and TSan builds use the system allocator
# (LUAJIT_USE_SYSMALLOC), and a GC64 state must lie in the low 47 bits of
# the address space. On arm64 Linux, malloc and the sanitizers' heaps both
# hand out addresses above that, so lua_newstate fails there. Those builds
# run on x64 Linux and on macOS.
case "$(uname -s)-$(uname -m)" in
  Linux-aarch64|Linux-arm64)
    if [ $ASAN = 1 ] || [ $TSAN = 1 ]; then
      echo "== sanitizer builds skipped: the system allocator is out of GC64's range on arm64 Linux"
    fi
    ASAN=0; TSAN=0 ;;
esac

# ASan and UBSan flags. Shifts are not checked. Alignment is not checked:
# upstream lj_parse.c reads a misaligned GCobj pointer by design. Clang's
# function check (calls through a cast function pointer) is off where the
# compiler has it; GCC does not.
NOSAN=shift,alignment
if $CC -fsanitize=undefined -fno-sanitize=function -c -x c /dev/null -o /dev/null 2>/dev/null; then
  NOSAN=$NOSAN,function
fi
SAN="-fsanitize=address,undefined -fno-sanitize=$NOSAN -fno-omit-frame-pointer"

FAILED=""
fail() { FAILED="$FAILED
  $1"; echo "FAILED: $1"; }

build() {  # build NAME [make vars...]
  name=$1; shift
  echo "== build $name"
  if ! "$ROOT/tests/vm/build-vm.sh" "$T/$name" "$@" >"$T/$name.log" 2>&1; then
    tail -30 "$T/$name.log"
    fail "build $name (see $T/$name.log)"
    return 1
  fi
}

# The identity compiler, with the stubs of the text and regex libraries
# (LuaJIT/src/lj_nlnotext.c): a libnlc without them.
$CC -O2 -fPIC -Wall -Wextra -I"$ROOT/LuaJIT/src" -c "$ROOT/tests/vm/support/nlc_identity.c" \
  -o "$T/nlc_identity.o" &&
$CC -O2 -fPIC -Wall -Wextra -I"$ROOT/LuaJIT/src" -c "$ROOT/LuaJIT/src/lj_nlnotext.c" \
  -o "$T/nlc_identity_notext.o" &&
ar rcs "$T/libnlc_identity.a" "$T/nlc_identity.o" "$T/nlc_identity_notext.o" ||
  { echo "cannot build the identity compiler"; exit 1; }

FAKE="NL_RUNTIME_DIR=$ROOT/tests/vm/fake_runtime NL_LIBNLC=$T/libnlc_identity.a"

for v in jit nojit assert; do
  case $v in
    jit) X="XCFLAGS="; D="CCDEBUG=" ;;
    nojit) X="XCFLAGS=-DLUAJIT_DISABLE_JIT"; D="CCDEBUG=" ;;
    assert) X="XCFLAGS=-DLUA_USE_ASSERT -DLUA_USE_APICHECK"; D="CCDEBUG=-g" ;;
  esac
  # shellcheck disable=SC2086
  build fake-$v $FAKE "$X" "$D" || continue
  B=$T/fake-$v/src
  for t in test_capi test_threads; do
    if ! $CC -g -O1 -Wall -Wextra -Wno-unused-parameter -I"$B" -I"$ROOT/tests/vm" \
         -o "$T/$t-$v" "$ROOT/tests/vm/$t.c" "$B/libluajit.a" "$T/libnlc_identity.a" $LIBS -lpthread; then
      fail "compile $t ($v)"; continue
    fi
  done
  echo "== test_capi ($v)"
  [ -x "$T/test_capi-$v" ] && { "$T/test_capi-$v" "$(mktemp -d "$T/tmp/capi.XXXXXX")" || fail "test_capi ($v)"; }
  echo "== test_events.lua ($v)"
  "$B/luajit" "$ROOT/tests/vm/test_events.lua" || fail "test_events.lua ($v)"
  if [ $v = jit ]; then
    for opt in -joff -Ohotloop=1; do
      echo "== test_events.lua (jit, $opt)"
      # shellcheck disable=SC2086
      "$B/luajit" $opt "$ROOT/tests/vm/test_events.lua" >"$T/events.out" 2>&1
      st=$?
      grep -v '^ok - ' "$T/events.out"
      [ $st = 0 ] || fail "test_events.lua ($opt)"
    done
  fi
  echo "== test_threads ($v)"
  [ -x "$T/test_threads-$v" ] && { "$T/test_threads-$v" || fail "test_threads ($v)"; }
done

if [ $ASAN = 1 ]; then
  # The host tools (buildvm, minilua) are built without sanitizers: buildvm's
  # offsetof idioms trip UBSan.
  if build asan $FAKE "XCFLAGS=-DLUAJIT_USE_SYSMALLOC -DLUA_USE_ASSERT -DLUA_USE_APICHECK -DLUAJIT_DISABLE_JIT" \
       "CC=$CC $SAN" "HOST_CC=$CC" "CCDEBUG=-g" "CCOPT=-O1"; then
    B=$T/asan/src
    export ASAN_OPTIONS="detect_leaks=0 abort_on_error=0 ${ASAN_OPTIONS:-}"
    export UBSAN_OPTIONS="halt_on_error=1 print_stacktrace=1 ${UBSAN_OPTIONS:-}"
    for t in test_capi test_threads; do
      # shellcheck disable=SC2086
      $CC $SAN -g -O1 -I"$B" -I"$ROOT/tests/vm" -o "$T/$t-asan" "$ROOT/tests/vm/$t.c" \
        "$B/libluajit.a" "$T/libnlc_identity.a" $LIBS -lpthread || fail "compile $t (asan)"
    done
    echo "== test_capi (ASan+UBSan)"
    "$T/test_capi-asan" "$(mktemp -d "$T/tmp/capi.XXXXXX")" >"$T/asan-capi.out" 2>&1
    st=$?; grep -v '^ok' "$T/asan-capi.out" | head -40
    { [ $st = 0 ] && ! grep -q 'runtime error\|Sanitizer' "$T/asan-capi.out"; } || fail "test_capi (ASan+UBSan)"
    echo "== test_events.lua (ASan+UBSan)"
    "$B/luajit" "$ROOT/tests/vm/test_events.lua" >"$T/asan-events.out" 2>&1
    st=$?; grep -v '^ok' "$T/asan-events.out" | head -40
    { [ $st = 0 ] && ! grep -q 'runtime error\|Sanitizer' "$T/asan-events.out"; } || fail "test_events.lua (ASan+UBSan)"
    echo "== test_threads (ASan+UBSan)"
    "$T/test_threads-asan" >"$T/asan-threads.out" 2>&1
    st=$?; grep -v '^ok' "$T/asan-threads.out" | head -40
    { [ $st = 0 ] && ! grep -q 'runtime error\|Sanitizer' "$T/asan-threads.out"; } || fail "test_threads (ASan+UBSan)"
    unset ASAN_OPTIONS UBSAN_OPTIONS
  fi
fi

if [ $TSAN = 1 ]; then
  # TSan instruments the C parts (the inbox among them). The interpreter's
  # assembler is not instrumented; the JIT is off so that no machine code is
  # generated at run time.
  if build tsan $FAKE "XCFLAGS=-DLUAJIT_DISABLE_JIT -DLUAJIT_USE_SYSMALLOC" \
       "CC=$CC -fsanitize=thread -g" "CCOPT=-O1 -fno-omit-frame-pointer"; then
    B=$T/tsan/src
    if $CC -fsanitize=thread -g -O1 -I"$B" -I"$ROOT/tests/vm" -o "$T/test_threads-tsan" \
         "$ROOT/tests/vm/test_threads.c" "$B/libluajit.a" "$T/libnlc_identity.a" $LIBS -lpthread; then
      echo "== test_threads (ThreadSanitizer)"
      TSAN_OPTIONS="halt_on_error=1 exitcode=66 ${TSAN_OPTIONS:-}" "$T/test_threads-tsan" ||
        fail "test_threads under ThreadSanitizer"
    else
      fail "compile test_threads with ThreadSanitizer"
    fi
  fi
fi

echo "== real compiler and runtime: test_capi_real"
"$ROOT/tests/vm/real/run.sh" || fail "test_capi_real"
# test_capi_real and test_sandbox again with LuaJIT's assertions and API
# checks: the fake-runtime assert build above cannot run them.
RNATIVE=""
case "$(uname -s)" in Linux) RNATIVE="-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc" ;; esac
cargo build -q -p nl_capi --release --manifest-path "$ROOT/Cargo.toml" &&
  build real-assert "NL_LIBNLC=$ROOT/target/release/libnl_capi.a" "NL_LIBNLC_LIBS=$RNATIVE" \
    "XCFLAGS=-DLUA_USE_ASSERT -DLUA_USE_APICHECK" "CCDEBUG=-g" && {
  B=$T/real-assert/src
  for t in test_capi_real test_sandbox test_transfer test_workers; do
    echo "== $t (real, assertions)"
    if $CC -g -Wall -Wno-unused-parameter -I"$B" -I"$ROOT/tests/vm" -o "$T/$t-assert" \
         "$ROOT/tests/vm/real/$t.c" "$B/libluajit.a" "$ROOT/target/release/libnl_capi.a" $RNATIVE $LIBS -lpthread; then
      "$T/$t-assert" "$(mktemp -d "$T/tmp/assert.XXXXXX")" >"$T/$t-assert.out" 2>&1 || fail "$t (assertions)"
      grep -v '^ok' "$T/$t-assert.out"
    else
      fail "compile $t (assertions)"
    fi
  done
}
if [ $ASAN = 1 ]; then
  # The transfer encoder and the workers with AddressSanitizer and
  # UndefinedBehaviorSanitizer, the real runtime and the release libnlc
  # (not instrumented): ports, requests and messages are freed on other
  # threads than the ones that made them.
  build real-asan "NL_LIBNLC=$ROOT/target/release/libnl_capi.a" "NL_LIBNLC_LIBS=$RNATIVE" \
    "XCFLAGS=-DLUAJIT_USE_SYSMALLOC -DLUA_USE_ASSERT -DLUA_USE_APICHECK -DLUAJIT_DISABLE_JIT" \
    "CC=$CC $SAN" "HOST_CC=$CC" "CCDEBUG=-g" "CCOPT=-O1" && {
    B=$T/real-asan/src
    export ASAN_OPTIONS="detect_leaks=0 abort_on_error=0 ${ASAN_OPTIONS:-}"
    export UBSAN_OPTIONS="halt_on_error=1 print_stacktrace=1 ${UBSAN_OPTIONS:-}"
    for t in test_transfer test_workers; do
      # shellcheck disable=SC2086
      if $CC $SAN -g -O1 -Wno-unused-parameter -I"$B" -I"$ROOT/tests/vm" -o "$T/$t-realasan" \
           "$ROOT/tests/vm/real/$t.c" "$B/libluajit.a" "$ROOT/target/release/libnl_capi.a" $RNATIVE $LIBS -lpthread; then
        echo "== $t (real, ASan+UBSan)"
        "$T/$t-realasan" "$(mktemp -d "$T/tmp/asan.XXXXXX")" >"$T/$t-realasan.out" 2>&1
        st=$?; grep -v '^ok' "$T/$t-realasan.out" | head -40
        { [ $st = 0 ] && ! grep -q 'runtime error\|Sanitizer' "$T/$t-realasan.out"; } || fail "$t (real, ASan+UBSan)"
      else
        fail "compile $t (real, ASan+UBSan)"
      fi
    done
    unset ASAN_OPTIONS UBSAN_OPTIONS
  }
fi
if [ $TSAN = 1 ]; then
  # The workers with ThreadSanitizer: the C side (the fork, its inboxes,
  # queues and ports, and the tests) instrumented, the real runtime, and
  # the release libnlc, which TSan cannot instrument on stable Rust. JIT
  # off, system allocator, as for the tsan build above. Reports from inside
  # the Rust library are suppressed (tests/vm/support/tsan-libnlc.supp).
  build real-tsan "NL_LIBNLC=$ROOT/target/release/libnl_capi.a" "NL_LIBNLC_LIBS=$RNATIVE" \
    "XCFLAGS=-DLUAJIT_DISABLE_JIT -DLUAJIT_USE_SYSMALLOC" \
    "CC=$CC -fsanitize=thread -g" "CCOPT=-O1 -fno-omit-frame-pointer" && {
    B=$T/real-tsan/src
    for t in real/test_workers test_threads; do
      n=$(basename $t)
      # shellcheck disable=SC2086
      if $CC -fsanitize=thread -g -O1 -Wno-unused-parameter -I"$B" -I"$ROOT/tests/vm" -o "$T/$n-realtsan" \
           "$ROOT/tests/vm/$t.c" "$B/libluajit.a" "$ROOT/target/release/libnl_capi.a" $RNATIVE $LIBS -lpthread; then
        echo "== $n (real, ThreadSanitizer)"
        case $n in test_workers) A="$(mktemp -d "$T/tmp/tsan.XXXXXX") --stress" ;; *) A="" ;; esac
        # shellcheck disable=SC2086
        TSAN_OPTIONS="halt_on_error=1 exitcode=66 suppressions=$ROOT/tests/vm/support/tsan-libnlc.supp ${TSAN_OPTIONS:-}" \
          "$T/$n-realtsan" $A >"$T/$n-realtsan.out" 2>&1 ||
          { tail -60 "$T/$n-realtsan.out"; fail "$n under ThreadSanitizer (real)"; }
        grep -v '^ok' "$T/$n-realtsan.out" | head -20
      else
        fail "compile $n with ThreadSanitizer (real)"
      fi
    done
  }
fi
echo "== real compiler and runtime: e2e"
"$ROOT/tests/vm/e2e/run.sh" || fail "e2e"
echo "== real compiler and runtime: conformance programs on the fork (worker programs with 0, 1 and 8 pool workers)"
"$ROOT/tests/vm/conformance.sh" || fail "conformance (JIT on)"
NL_CONFORM_JIT=off "$ROOT/tests/vm/conformance.sh" || fail "conformance (JIT off)"
NL_CONFORM_JIT=hot "$ROOT/tests/vm/conformance.sh" || fail "conformance (JIT forced)"
echo "== real compiler and runtime: regex and string libraries (tests/vm/text)"
"$ROOT/tests/vm/text/run.sh" || fail "text (JIT on)"
NL_CONFORM_JIT=off "$ROOT/tests/vm/text/run.sh" || fail "text (JIT off)"
NL_TEXT_ASSERT=1 "$ROOT/tests/vm/text/run.sh" || fail "text (assertions)"
echo "== real compiler and runtime: NL opcodes (tests/vm/nlops)"
"$ROOT/tests/vm/nlops/run.sh" || fail "nlops"
echo "== Rust: nl_sys links against the fork (tests/vm/nl_sys_link)"
"$ROOT/tests/vm/nl_sys_link/run.sh" --stock || fail "nl_sys_link"
echo "== Rust: the nl crate (tests/vm/nl_host)"
"$ROOT/tests/vm/nl_host/run.sh" || fail "nl_host"
echo "== Rust: an example host from a fresh crate (tests/vm/nl_example)"
"$ROOT/tests/vm/nl_example/run.sh" || fail "nl_example"

if [ -n "$FAILED" ]; then
  echo "tests/vm: FAILED:$FAILED"
  exit 1
fi
echo "tests/vm: all passed"
