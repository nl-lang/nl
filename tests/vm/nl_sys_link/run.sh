#!/bin/sh
# Builds and runs the nl_sys link test against the fork (see Cargo.toml).
#   tests/vm/nl_sys_link/run.sh            link against the fork; run
#   tests/vm/nl_sys_link/run.sh --stock    also check that linking against
#                                          stock LuaJIT (no patch) fails
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
# A copy of luajit-src-rs that builds the fork (tests/vm/support).
"$ROOT/tests/vm/support/prepare_luajit_src.sh" "$ROOT/target/nl_sys_link/luajit-src"

cd "$HERE"
export CARGO_TARGET_DIR="$ROOT/target/nl_sys_link/target"
echo "-- cargo run (fork through [patch.crates-io] luajit-src)"
cargo run -q
if [ "${1:-}" = "--stock" ]; then
  echo "-- cargo build without the patch (stock luajit-src): must fail to link"
  STOCKDIR=$ROOT/target/nl_sys_link/stock
  mkdir -p "$STOCKDIR/src" "$STOCKDIR/.cargo"
  sed -e '/^\[patch.crates-io\]/,$d' -e 's#"\.\./\.\./\.\./crates/nl_sys"#"'"$ROOT"'/crates/nl_sys"#' \
    Cargo.toml >"$STOCKDIR/Cargo.toml"
  cp src/main.rs "$STOCKDIR/src/"
  if (cd "$STOCKDIR" && CARGO_TARGET_DIR="$ROOT/target/nl_sys_link/stock-target" \
        cargo build -q) >"$ROOT/target/nl_sys_link/stock.log" 2>&1; then
    echo "not ok - a build against stock LuaJIT linked"
    exit 1
  fi
  if grep -q 'nl_version_0_1' "$ROOT/target/nl_sys_link/stock.log"; then
    echo "ok - the build against stock LuaJIT fails to link, naming nl_version_0_1:"
    grep -m3 'nl_version_0_1\|Undefined symbols\|undefined reference' "$ROOT/target/nl_sys_link/stock.log" | sed 's/^/    /'
  else
    echo "not ok - the stock build failed, but not on nl_version_0_1:"
    tail -20 "$ROOT/target/nl_sys_link/stock.log"
    exit 1
  fi
fi
