#!/bin/sh
# An example host on the nl crate (src/main.rs, main.nl) builds
# from a fresh crate with only the nl dependency and the patch of luajit-src,
# and runs. Twice: this crate (tests/vm/nl_example), and one that `cargo new`
# makes outside the repository, given the same src/main.rs and main.nl and
# the two entries of Cargo.toml. Both print expected.out.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
T=$ROOT/target/nl_example
STATUS=0
mkdir -p "$T"
"$ROOT/tests/vm/support/prepare_luajit_src.sh" "$T/luajit-src" || exit 1
export CARGO_TARGET_DIR="$T/target"

check() {  # check NAME DIR
  if (cd "$2" && cargo run -q) >"$T/$1.out" 2>"$T/$1.err" && cmp -s "$T/$1.out" "$HERE/expected.out"; then
    echo "ok - $1: the example builds and runs"
  else
    echo "not ok - $1:"; cat "$T/$1.out"; tail -30 "$T/$1.err"
    STATUS=1
  fi
}

check nl_example "$HERE"

FRESH=$(mktemp -d "${TMPDIR:-/tmp}/nl_example.XXXXXX")
(cd "$FRESH" && cargo new -q --vcs none fresh_host) || { echo "not ok - cargo new"; exit 1; }
cat >>"$FRESH/fresh_host/Cargo.toml" <<TOML
nl = { path = "$ROOT/crates/nl", features = ["async"] }

[patch.crates-io]
luajit-src = { path = "$T/luajit-src" }
TOML
cp "$HERE/src/main.rs" "$FRESH/fresh_host/src/main.rs"
cp "$HERE/main.nl" "$FRESH/fresh_host/main.nl"
check fresh_host "$FRESH/fresh_host"
rm -rf "$FRESH"

[ $STATUS = 0 ] && echo "nl_example: passed" || echo "nl_example: FAILED"
exit $STATUS
