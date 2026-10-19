#!/bin/sh
# The tests of the nl crate (crates/nl) against the VM fork, with the real
# compiler and the real runtime:
#   1. cargo test in this crate (tests/*.rs), mlua and nl linked against the
#      fork through [patch.crates-io] luajit-src (see Cargo.toml);
#   2. the complete C embedding example embedding/host.c, built against the
#      fork and the release libnlc as tests/vm/real/run.sh builds them, and
#      its Rust twin (src/bin/embedding.rs) run on the same NL files
#      (embedding/scripts): their stdout and stderr must be the same;
#   3. the C worker host workers/host.c and its Rust twin src/bin/workers.rs,
#      with 0, 1 and 4 pool workers: the same output, and the same for every
#      pool size.
# Then the tests that make sense with mlua's `send` feature, once more with it.
# Everything twice: the VM with the JIT on, and built with
# -DLUAJIT_DISABLE_JIT (luajit-src-nojit, target-nojit; the C hosts against
# target/vm/real-nojit).
# Exit status 0 when everything passed.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
T=$ROOT/target/nl_host
CC=${CC:-cc}
case "$(uname -s)" in
  Darwin) : "${MACOSX_DEPLOYMENT_TARGET:=11.0}"; export MACOSX_DEPLOYMENT_TARGET; NATIVE="" ;;
  Linux)  NATIVE="-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc" ;;
  *)      NATIVE="" ;;
esac
STATUS=0
mkdir -p "$T"
"$ROOT/tests/vm/support/prepare_luajit_src.sh" "$T/luajit-src" || exit 1
"$ROOT/tests/vm/support/prepare_luajit_src.sh" "$T/luajit-src-nojit" --nojit || exit 1
cargo build -q -p nl_capi --release --manifest-path "$ROOT/Cargo.toml" || { echo "cannot build libnlc"; exit 1; }
LIBNLC=$ROOT/target/release/libnl_capi.a

for v in jit nojit; do
  if [ $v = jit ]; then
    TGT=$T/target; PATCH=""; XC="XCFLAGS="
  else
    # The same crate against the VM built without the JIT: the patch of
    # Cargo.toml is replaced on the command line.
    TGT=$T/target-nojit; PATCH="patch.crates-io.luajit-src.path=\"$T/luajit-src-nojit\""; XC="XCFLAGS=-DLUAJIT_DISABLE_JIT"
  fi
  cargo_() { if [ -n "$PATCH" ]; then cargo "$@" --config "$PATCH"; else cargo "$@"; fi; }

  echo "-- cargo test (nl against the fork, $v)"
  (cd "$HERE" && NL_HOST_JIT=$([ $v = jit ] && echo on || echo off) CARGO_TARGET_DIR=$TGT cargo_ test -q) >"$T/test-$v.out" 2>&1
  st=$?
  # Not the panic messages of worker threads ('<unnamed>'): the panic tests
  # print them on purpose.
  grep -E "^(test result: [^o]|---- |failures:|thread '[^<])" "$T/test-$v.out"
  echo "$(grep -c '^test result: ok' "$T/test-$v.out") test binaries ok, $(grep '^test result' "$T/test-$v.out" |
    sed 's/.*ok\. \([0-9]*\) passed.*/\1/' | awk '{n += $1} END {print n + 0}') tests passed"
  if [ $st != 0 ]; then tail -40 "$T/test-$v.out"; STATUS=1; fi

  # The C hosts, built against the fork and the release libnlc as
  # tests/vm/real/run.sh builds them.
  if ! "$ROOT/tests/vm/build-vm.sh" "$ROOT/target/vm/real-$v" "NL_LIBNLC=$LIBNLC" "NL_LIBNLC_LIBS=$NATIVE" "$XC" \
       >"$T/real-$v.log" 2>&1; then
    tail -20 "$T/real-$v.log"; echo "build real-$v failed"; exit 1
  fi
  B=$ROOT/target/vm/real-$v/src
  (cd "$HERE" && CARGO_TARGET_DIR=$TGT cargo_ build -q --bins) >"$T/build-$v.out" 2>&1 ||
    { cat "$T/build-$v.out"; echo "not ok - the Rust examples do not build ($v)"; exit 1; }

  # compare NAME DIR CMD-C CMD-RUST: both run in DIR, same stdout and stderr.
  compare() {
    (cd "$2" && $3 >"$T/c.out" 2>"$T/c.err") || { echo "not ok - $1: the C host failed"; STATUS=1; }
    (cd "$2" && $4 >"$T/rust.out" 2>"$T/rust.err") || { echo "not ok - $1: the Rust host failed"; STATUS=1; }
    if [ -s "$T/c.out" ] && cmp -s "$T/c.out" "$T/rust.out" && cmp -s "$T/c.err" "$T/rust.err"; then
      echo "ok - $1: the C and Rust hosts print the same ($(wc -l <"$T/c.out" | tr -d ' ') lines)"
    else
      echo "not ok - $1: the C and Rust hosts differ:"
      diff "$T/c.out" "$T/rust.out"; diff "$T/c.err" "$T/rust.err"
      STATUS=1
    fi
  }

  rm -rf "$T/embedding-$v"
  # shellcheck disable=SC2086
  $CC -g -Wall -Wno-unused-parameter -I"$B" -o "$T/embedding-$v" "$HERE/embedding/host.c" "$B/libluajit.a" "$LIBNLC" $NATIVE -lm ||
    { echo "not ok - the C example does not build"; exit 1; }
  compare "embedding example ($v)" "$HERE/embedding" "$T/embedding-$v" "$TGT/debug/embedding"

  # shellcheck disable=SC2086
  $CC -g -Wall -Wno-unused-parameter -I"$B" -o "$T/workers-$v" "$HERE/workers/host.c" "$B/libluajit.a" "$LIBNLC" $NATIVE -lm ||
    { echo "not ok - the C worker host does not build"; exit 1; }
  for n in 0 1 4; do
    compare "workers, $n pool workers ($v)" "$HERE/workers" "$T/workers-$v $n" "$TGT/debug/workers $n"
    if [ $n = 0 ]; then cp "$T/c.out" "$T/workers0.out"
    elif ! cmp -s "$T/c.out" "$T/workers0.out"; then echo "not ok - $n pool workers print other output than 0"; STATUS=1; fi
  done
done

# mlua's `send` feature: the test files whose closures can be Send (the
# others keep state in an Rc, as a host without `send` does), and tests/send.rs,
# which only runs with it. The examples are built without it.
SEND_TESTS="types threads loading futures workers smoke send"
echo "-- cargo test --features send (nl against the fork, jit): $SEND_TESTS"
(cd "$HERE" && CARGO_TARGET_DIR=$T/target cargo test -q --features send $(for t in $SEND_TESTS; do printf -- '--test %s ' "$t"; done)) \
  >"$T/test-send.out" 2>&1
st=$?
grep -E "^(test result: [^o]|---- |failures:|thread '[^<])" "$T/test-send.out"
echo "$(grep -c '^test result: ok' "$T/test-send.out") test binaries ok, $(grep '^test result' "$T/test-send.out" |
  sed 's/.*ok\. \([0-9]*\) passed.*/\1/' | awk '{n += $1} END {print n + 0}') tests passed"
if [ $st != 0 ]; then tail -40 "$T/test-send.out"; STATUS=1; fi

[ $STATUS = 0 ] && echo "nl_host: passed" || echo "nl_host: FAILED"
exit $STATUS
