#!/bin/sh
# Compiles every .lua file under the given directories as GLua
# (NL_OPT_GLUA) and lists the ones that do not compile. The intended corpus is
# a checkout of https://github.com/Facepunch/garrysmod:
#
#   git clone --depth 1 --filter=blob:none --sparse https://github.com/Facepunch/garrysmod
#   (cd garrysmod && git sparse-checkout set garrysmod/lua garrysmod/gamemodes)
#   tests/vm/glua_corpus.sh garrysmod/garrysmod/lua garrysmod/garrysmod/gamemodes
#
# Uses the JIT build of tests/vm/run.sh (target/vm/fake-jit) and builds it
# when it is missing. Exit status 0 when every file compiles.
set -eu

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
T=$ROOT/target/vm
B=$T/fake-jit/src
CC=${CC:-cc}
LIBS="-lm -lpthread"
case "$(uname -s)" in Linux) LIBS="-lm -ldl -lpthread" ;; esac
[ $# -gt 0 ] || { echo "usage: $0 DIR..." >&2; exit 2; }

mkdir -p "$T"
if [ ! -f "$B/libluajit.a" ]; then
  $CC -O2 -fPIC -I"$ROOT/LuaJIT/src" -c "$ROOT/tests/vm/support/nlc_identity.c" -o "$T/nlc_identity.o"
  ar rcs "$T/libnlc_identity.a" "$T/nlc_identity.o"
  "$ROOT/tests/vm/build-vm.sh" "$T/fake-jit" "NL_RUNTIME_DIR=$ROOT/tests/vm/fake_runtime" \
    "NL_LIBNLC=$T/libnlc_identity.a" >"$T/fake-jit.log" 2>&1 ||
    { tail -20 "$T/fake-jit.log"; exit 1; }
fi
$CC -O1 -Wall -Wextra -I"$B" -o "$T/glua_check" "$ROOT/tests/vm/support/glua_check.c" \
  "$B/libluajit.a" "$T/libnlc_identity.a" $LIBS 2>&1 | grep -v '^ld: warning' || true
[ -x "$T/glua_check" ] || { echo "cannot build glua_check" >&2; exit 1; }

LIST=$T/glua_corpus.txt
find "$@" -name '*.lua' | sort >"$LIST"
echo "glua_corpus: $(wc -l <"$LIST" | tr -d ' ') files"
if xargs "$T/glua_check" -g -q <"$LIST"; then
  echo "glua_corpus: all compile"
else
  echo "glua_corpus: FAILED"
  exit 1
fi
