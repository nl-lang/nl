#!/bin/sh
# test_capi_real: the C API against the real runtime (runtime/*.lua) and the
# real compiler (crates/nl_capi), JIT on and off. Builds what it needs under
# target/vm (real-jit, real-nojit; with NL_REAL_BUILD=name, name-jit and
# name-nojit), as tests/vm/e2e/run.sh does.
# test_sandbox: the sandbox profile (NL_OPT_SANDBOX), and the conformance
# programs of SANDBOX_PROGRAMS run under it (tests/vm/conformance.sh with
# NL_CONFORM_SANDBOX=1). Programs that use IO, OS, Debug, Package or Jit
# cannot run there, nor worker_send_messages (it makes a userdata with newproxy).
# test_transfer: the transfer encoder between two states; test_workers:
# workers with 0, 1 and N pool threads, dedicated workers, the host API of
# workers and a stress test.
set -u
SANDBOX_PROGRAMS="ctrl_case_patterns conv_custom err_finally err_raise_catch
  event_basic event_reload expr_ranges fn_closure iface_basic import_add_path
  import_include import_named lua_interop lua_load lua_paths op_number_operator
  prim_methods regex_basic string_format struct_inherit struct_new_defaults
  struct_new_errors struct_new_import struct_new_site task_basic task_combine
  type_checks var_global worker_copy worker_dedicated worker_register
  worker_run_map worker_send_error worker_spawn_block worker_spawn_call
  worker_spawn_forms worker_type_values"
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
T=$ROOT/target/vm
CC=${CC:-cc}
case "$(uname -s)" in
  Darwin) : "${MACOSX_DEPLOYMENT_TARGET:=11.0}"; export MACOSX_DEPLOYMENT_TARGET; NATIVE="" ;;
  Linux)  NATIVE="-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc" ;;
  *)      NATIVE="" ;;
esac
P=${NL_REAL_BUILD:-real}
mkdir -p "$T/tmp"
STATUS=0
cargo build -q -p nl_capi --release --manifest-path "$ROOT/Cargo.toml" || { echo "cannot build libnlc"; exit 1; }
LIBNLC=$ROOT/target/release/libnl_capi.a
for v in jit nojit; do
  if [ $v = nojit ]; then X="XCFLAGS=-DLUAJIT_DISABLE_JIT"; else X="XCFLAGS="; fi
  if ! "$ROOT/tests/vm/build-vm.sh" "$T/$P-$v" "NL_LIBNLC=$LIBNLC" "NL_LIBNLC_LIBS=$NATIVE" "$X" \
       >"$T/$P-$v.log" 2>&1; then
    tail -20 "$T/$P-$v.log"; echo "build $P-$v failed"; STATUS=1; continue
  fi
  B=$T/$P-$v/src
  # shellcheck disable=SC2086
  $CC -g -Wall -Wno-unused-parameter -I"$B" -I"$ROOT/tests/vm" -o "$T/test_capi_real-$v" \
    "$ROOT/tests/vm/real/test_capi_real.c" "$B/libluajit.a" "$LIBNLC" $NATIVE -lm ||
    { echo "cannot build test_capi_real"; STATUS=1; continue; }
  echo "-- test_capi_real ($v)"
  "$T/test_capi_real-$v" "$(mktemp -d "$T/tmp/real.XXXXXX")" >"$T/real-$v.out" 2>&1 || STATUS=1
  grep -v '^ok' "$T/real-$v.out"
  # shellcheck disable=SC2086
  $CC -g -Wall -Wno-unused-parameter -I"$B" -I"$ROOT/tests/vm" -o "$T/test_sandbox-$v" \
    "$ROOT/tests/vm/real/test_sandbox.c" "$B/libluajit.a" "$LIBNLC" $NATIVE -lm ||
    { echo "cannot build test_sandbox"; STATUS=1; continue; }
  echo "-- test_sandbox ($v)"
  "$T/test_sandbox-$v" "$(mktemp -d "$T/tmp/sandbox.XXXXXX")" >"$T/sandbox-$v.out" 2>&1 || STATUS=1
  grep -v '^ok' "$T/sandbox-$v.out"
  for t in test_transfer test_workers; do
    # shellcheck disable=SC2086
    $CC -g -Wall -Wno-unused-parameter -I"$B" -I"$ROOT/tests/vm" -o "$T/$t-$v" \
      "$ROOT/tests/vm/real/$t.c" "$B/libluajit.a" "$LIBNLC" $NATIVE -lm -lpthread ||
      { echo "cannot build $t"; STATUS=1; continue; }
    echo "-- $t ($v)"
    "$T/$t-$v" "$(mktemp -d "$T/tmp/$t.XXXXXX")" >"$T/$t-$v.out" 2>&1 || STATUS=1
    grep -v '^ok' "$T/$t-$v.out"
  done
  echo "-- conformance programs under the sandbox ($v)"
  if [ $v = nojit ]; then J=off; else J=on; fi
  # shellcheck disable=SC2086
  NL_CONFORM_SANDBOX=1 NL_CONFORM_JIT=$J NL_CONFORM_BUILD=$P-$v \
    "$ROOT/tests/vm/conformance.sh" $(for p in $SANDBOX_PROGRAMS; do printf '%s.nl ' "$p"; done) ||
    STATUS=1
done
exit $STATUS
