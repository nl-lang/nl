#!/bin/sh
# Minifies every .lua file under the given directories as GLua
# (`nlc minify --glua`, crates/nl_capi/src/minify.rs) and checks that the
# result is the same program on the same lines: loaded with GLua's parser
# (NL_OPT_GLUA), the minified file must give the same bytecode dump, debug
# information included, as the file itself (tests/vm/support/glua_dump.c,
# which fixes the VM's hash seed so that dumps compare). The intended
# corpus is the one of glua_corpus.sh, a checkout of
# https://github.com/Facepunch/garrysmod:
#
#   tests/vm/minify_corpus.sh garrysmod/garrysmod/lua garrysmod/garrysmod/gamemodes
#
# Files that do not load as they are are counted and skipped. Uses the JIT
# build of tests/vm/run.sh (target/vm/fake-jit), built by glua_corpus.sh
# when missing, and builds nlc. Exit status 0 when every file that loads
# minifies to the same dump.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
T=$ROOT/target/vm
B=$T/fake-jit/src
CC=${CC:-cc}
LIBS="-lm -lpthread"
case "$(uname -s)" in Linux) LIBS="-lm -ldl -lpthread" ;; esac
[ $# -gt 0 ] || { echo "usage: $0 DIR..." >&2; exit 2; }
if [ ! -f "$B/libluajit.a" ] || [ ! -f "$T/libnlc_identity.a" ]; then
  # glua_corpus.sh builds both; given no files, it only builds.
  mkdir -p "$T/empty"
  "$ROOT/tests/vm/glua_corpus.sh" "$T/empty" >/dev/null
fi
$CC -O1 -Wall -Wextra -I"$B" -o "$T/glua_dump" "$ROOT/tests/vm/support/glua_dump.c" \
  "$B/libluajit.a" "$T/libnlc_identity.a" $LIBS 2>&1 | grep -v '^ld: warning' || true
[ -x "$T/glua_dump" ] || { echo "cannot build glua_dump" >&2; exit 1; }
(cd "$ROOT" && cargo build -q -p nlc)
NLC=$ROOT/target/debug/nlc
W=$T/minify_corpus
rm -rf "$W"
mkdir -p "$W"
LIST=$W/files.txt
find "$@" -name '*.lua' | sort >"$LIST"
total=0 same=0 skipped=0 failed=0 raw=0 small=0
while IFS= read -r f; do
  total=$((total + 1))
  if ! "$T/glua_dump" "$f" "$W/a.bc" >/dev/null 2>&1; then skipped=$((skipped + 1)); continue; fi
  if ! "$NLC" minify --glua -o "$W/m.lua" "$f" 2>"$W/err"; then
    echo "FAIL  $f: nlc minify: $(cat "$W/err")"; failed=$((failed + 1)); continue
  fi
  if ! "$T/glua_dump" "$W/m.lua" "$W/b.bc" >"$W/err" 2>&1; then
    echo "FAIL  $f: the minified file does not load: $(head -c 300 "$W/err")"; failed=$((failed + 1)); continue
  fi
  if cmp -s "$W/a.bc" "$W/b.bc"; then
    same=$((same + 1))
    raw=$((raw + $(wc -c <"$f")))
    small=$((small + $(wc -c <"$W/m.lua")))
  else
    echo "FAIL  $f: the minified file compiles to other bytecode"; failed=$((failed + 1))
  fi
done <"$LIST"
echo "minify_corpus: $total files, $same the same once minified ($raw bytes to $small), $skipped not loading as they are, $failed failed"
[ "$failed" = 0 ]
