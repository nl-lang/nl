#!/bin/sh
# NL programs compiled with the portable profile, run on the stock LuaJIT
# builds that Garry's Mod clients use, under a mock of gmod's Lua
# environment (tests/gmod/mock.lua).
#
#   tests/gmod/run.sh [--no-glua] [--no-build] [--only=2.0.4|2.1.0-beta3] [name...]
#
# 1. Builds nlc and bundles (`nlc bundle`) every conformance program of
#    tests/run that the profile allows (EXCLUDED below says why the others
#    are left out) and every program of tests/gmod, into target/gmod/bundles.
# 2. Builds stock LuaJIT 2.0.4 (the 32-bit x86 VM of gmod's main branch)
#    and 2.1.0-beta3 (the x86-64 branch's) from their release commits,
#    which the LuaJIT/ clone has in its history (git archive), into
#    target/stock-luajit, unless they are there already. On a machine that
#    cannot build or run them itself (2.0.4 has no arm64 port, and Rosetta
#    cannot give a non-GC64 VM its low memory), they are built and run in
#    Docker: linux/386 for 2.0.4, linux/amd64 for 2.1.0-beta3 (with GC64).
#    NL_GMOD_DOCKER=never forbids that, =always forces it. On x86-64
#    Linux, 2.0.4 is built for 32-bit x86 too: with gcc -m32 when it can
#    link a 32-bit program, else in Docker (linux/386), else for x86-64,
#    which the run reports. --no-build skips a VM that is not built yet
#    instead of building it.
# 3. Runs every bundle on both VMs, in the client realm, and the programs
#    of tests/gmod in the server realm as well, and compares stdout with
#    the .out file (stderr is ignored; the exit status must be 0). The
#    programs of tests/gmod are about gmod's environment (hook, timers,
#    classes); they are compiled with the declarations of tests/gmod/decl.
# 4. Unless --no-glua: compiles every bundle, and every chunk a bundle
#    holds as a string, with GLua's parser (the fork's NL_OPT_GLUA,
#    tests/vm/glua_corpus.sh), so the output parses in gmod as well as in
#    stock Lua.
#
# A name argument selects the programs whose name contains it. Exit status
# 0 when everything selected passes. A VM that cannot be had on this
# machine is reported as skipped, which does not fail the run.
set -u

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
T=$ROOT/target/gmod
SL=$ROOT/target/stock-luajit
NLC=$ROOT/target/debug/nlc
GLUA=1
BUILD=1
ONLY=
FILTERS=
for a in "$@"; do
  case "$a" in
    --no-glua) GLUA=0 ;;
    --no-build) BUILD=0 ;;
    --only=*) ONLY=${a#--only=} ;;
    -*) echo "unknown option $a" >&2; exit 2 ;;
    *) FILTERS="$FILTERS $a" ;;
  esac
done

# Conformance programs left out, and why.
EXCLUDED="
regex_* the regex engine is not in the portable profile
worker_dedicated Worker.new is not in the portable profile
worker_send_messages uses Regex as a value
worker_spawn_block reads a file-level variable that each worker has a copy of (as on every stock LuaJIT)
lua_load load_nl is not in the portable profile
import_add_path add_path is not in the portable profile
event_named relies on handlers running in the order they were added, which gmod's hook library does not keep
event_return relies on that order, and on a handler that returns nil ending the run, which it does not in gmod
lua_prelude uses IO, and gmod has no io library
type_is uses IO, and gmod has no io library
prim_plain_call uses loadstring, which gmod does not have
"

excluded() {
  echo "$EXCLUDED" | while read -r pat why; do
    [ -n "$pat" ] || continue
    case "$1" in $pat) echo "$why"; return ;; esac
  done
}

selected() {
  [ -z "$FILTERS" ] && return 0
  for pattern in $FILTERS; do
    case "$1" in *"$pattern"*) return 0 ;; esac
  done
  return 1
}

fail=0
rm -rf "$T/bundles" "$T/out"
mkdir -p "$T/bundles" "$T/out"

echo "== build nlc"
(cd "$ROOT" && cargo build -q -p nlc) || exit 1

# The list of runs: one line per run, "name dir realm".
LIST=$T/runs.txt
: >"$LIST"
skipped=0
echo "== bundle"
bundle() { # name dir file decl realms...
  name=$1 dir=$2 file=$3 decl=$4
  shift 4
  if (cd "$dir" && "$NLC" bundle ${decl:+--decl "$decl"} -o "$T/bundles/$name.lua" "$file") >"$T/out/$name.bundle.err" 2>&1; then
    for realm in "$@"; do echo "$name $dir $realm" >>"$LIST"; done
  else
    echo "FAIL  $name: nlc bundle: $(grep -m1 'error' "$T/out/$name.bundle.err")"
    fail=1
  fi
}
for f in "$ROOT"/tests/run/*.nl; do
  name=$(basename "$f" .nl)
  [ -f "${f%.nl}.out" ] || continue
  selected "$name" || continue
  why=$(excluded "$name")
  if [ -n "$why" ]; then skipped=$((skipped + 1)); continue; fi
  bundle "$name" "$ROOT/tests/run" "$name.nl" "" client
done
for f in "$ROOT"/tests/gmod/*.nl; do
  [ -f "$f" ] || continue
  name=gmod_$(basename "$f" .nl)
  [ -f "${f%.nl}.out" ] || continue
  selected "$name" || continue
  bundle "$name" "$ROOT/tests/gmod" "$(basename "$f")" decl client server
done
echo "$(wc -l <"$LIST" | tr -d ' ') runs per VM; $skipped conformance programs left out (tests/gmod/run.sh, EXCLUDED)"

# --- the stock VMs ---------------------------------------------------------

# Release commits in LuaJIT's history; its public repository no longer has
# the tags.
COMMIT_204=69e5342e
COMMIT_210B3=8271c643

ARCH=$(uname -m)
docker_usable() { command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; }
case "${NL_GMOD_DOCKER:-auto}" in
  always) DOCKER=1 ;;
  never) DOCKER=0 ;;
  *)
    case "$(uname -s)/$ARCH" in
      Linux/x86_64 | Linux/i?86) DOCKER=0 ;;
      *) if docker_usable; then DOCKER=1; else DOCKER=0; fi ;;
    esac ;;
esac
DOCKER_DEFAULT=$DOCKER
# The C compiler for a native build (empty: the Makefile's own), and the
# suffix of its build directory.
VMCC=
NATIVE=native

# Whether gcc -m32 links a program that uses libm.
m32_links() {
  mkdir -p "$T"
  printf '#include <math.h>\nint main(int c, char **v) { (void)v; return (int)sqrt((double)c) - 1; }\n' >"$T/m32.c"
  gcc -m32 "$T/m32.c" -o "$T/m32" -lm >/dev/null 2>&1 && "$T/m32"
}

source_of() { # version commit
  src=$SL/src-$1
  if [ ! -f "$src/src/luajit.h" ]; then
    rm -rf "$src"
    mkdir -p "$src"
    if ! git -C "$ROOT/LuaJIT" cat-file -e "$2^{commit}" 2>/dev/null; then
      git -C "$ROOT/LuaJIT" fetch -q https://github.com/LuaJIT/LuaJIT.git v2.1 || return 1
    fi
    git -C "$ROOT/LuaJIT" archive "$2" | tar -x -C "$src" || return 1
  fi
}

# A Docker image with a C toolchain, made once per platform.
docker_image() { # platform
  img=nl-luajit-builder:$1
  if ! docker image inspect "$img" >/dev/null 2>&1; then
    docker rm -f "nl-luajit-builder-$1" >/dev/null 2>&1
    for _ in 1 2 3; do
      docker run --name "nl-luajit-builder-$1" --platform "linux/$1" alpine:3.22 \
        sh -c 'apk add --no-cache build-base >/dev/null' >/dev/null 2>&1 &&
        docker commit "nl-luajit-builder-$1" "$img" >/dev/null && break
      docker rm -f "nl-luajit-builder-$1" >/dev/null 2>&1
      sleep 3
    done
    docker rm -f "nl-luajit-builder-$1" >/dev/null 2>&1
    docker image inspect "$img" >/dev/null 2>&1 || return 1
  fi
  echo "$img"
}

# Runs a command natively, or in Docker on `platform` with the worktree
# mounted at the same path.
in_vm_env() { # platform command...
  platform=$1
  shift
  if [ "$DOCKER" = 1 ]; then
    img=$(docker_image "$platform") || { echo "cannot make the Docker image for linux/$platform" >&2; return 1; }
    # On Linux, as this user: the build and its output land in the worktree.
    user=
    [ "$(uname -s)" = Linux ] && user="--user=$(id -u):$(id -g)"
    docker run --rm $user --platform "linux/$platform" -v "$ROOT:$ROOT" -w "$ROOT" "$img" "$@"
  else
    "$@"
  fi
}

# Builds a VM unless it is there; prints the path of its luajit binary.
build_vm() { # version commit platform make-args...
  version=$1 commit=$2 platform=$3
  shift 3
  out=$SL/build-$version-$platform
  [ "$DOCKER" = 1 ] || out=$SL/build-$version-$NATIVE
  if [ ! -x "$out/src/luajit" ]; then
    [ "$BUILD" = 1 ] || { echo "LuaJIT $version is not built ($out)" >&2; return 2; }
    source_of "$version" "$commit" || { echo "cannot get the source of LuaJIT $version" >&2; return 1; }
    rm -rf "$out"
    cp -R "$SL/src-$version" "$out"
    in_vm_env "$platform" make -C "$out" -j4 ${VMCC:+"CC=$VMCC"} "$@" >"$out.log" 2>&1 || { echo "cannot build LuaJIT $version (log: $out.log)" >&2; return 1; }
  fi
  echo "$out/src/luajit"
}

can_build() { # version
  [ "$DOCKER" = 1 ] && return 0
  case "$1/$ARCH" in
    2.0.4/x86_64 | 2.0.4/i?86 | 2.1.0-beta3/x86_64 | 2.1.0-beta3/i?86 | 2.1.0-beta3/aarch64) return 0 ;;
  esac
  return 1
}

run_on() { # version luajit platform
  version=$1 luajit=$2 platform=$3
  od=$T/out/$version
  mkdir -p "$od"
  # One shell for all the runs: Docker starts once.
  in_vm_env "$platform" sh -c '
    luajit=$1 list=$2 od=$3 mock=$4 bundles=$5
    while read -r name dir realm; do
      flag=--client
      [ "$realm" = server ] && flag=--server
      (cd "$dir" && "$luajit" "$mock" $flag "$bundles/$name.lua") >"$od/$name.$realm.got" 2>"$od/$name.$realm.err"
      echo $? >"$od/$name.$realm.code"
    done <"$list"
  ' sh "$luajit" "$LIST" "$od" "$ROOT/tests/gmod/mock.lua" "$T/bundles"
  pass=0 failed=0
  while read -r name dir realm; do
    case "$name" in
      gmod_*) expected=$dir/${name#gmod_}.out ;;
      *) expected=$dir/$name.out ;;
    esac
    code=$(cat "$od/$name.$realm.code" 2>/dev/null || echo "?")
    if [ "$code" = 0 ] && cmp -s "$expected" "$od/$name.$realm.got"; then
      pass=$((pass + 1))
    else
      failed=$((failed + 1))
      why=$(grep -m1 -v '^\s*$' "$od/$name.$realm.err" 2>/dev/null | cut -c1-200)
      [ "$code" = 0 ] && why="stdout differs: $(diff "$expected" "$od/$name.$realm.got" | sed -n 2p | cut -c1-160)"
      echo "FAIL  $version $realm $name: exit $code: $why"
    fi
  done <"$LIST"
  echo "LuaJIT $version: $pass passed, $failed failed"
  [ "$failed" = 0 ] || fail=1
}

for v in 2.0.4 2.1.0-beta3; do
  [ -z "$ONLY" ] || [ "$ONLY" = "$v" ] || continue
  echo "== LuaJIT $v"
  DOCKER=$DOCKER_DEFAULT VMCC= NATIVE=native
  # gmod ships 2.0.4 for 32-bit x86, which an x86-64 Linux machine can run.
  if [ "$v" = 2.0.4 ] && [ "$DOCKER" = 0 ] && [ "$(uname -s)/$ARCH" = Linux/x86_64 ]; then
    if m32_links; then
      VMCC="gcc -m32" NATIVE=native-x86
    elif [ "${NL_GMOD_DOCKER:-auto}" = auto ] && docker_usable; then
      DOCKER=1
    else
      echo "NOTE  LuaJIT $v is built for x86-64, not 32-bit x86: gcc -m32 cannot link (no 32-bit C library) and Docker is not used"
    fi
  fi
  if ! can_build "$v"; then
    echo "SKIP  LuaJIT $v: cannot be built on $(uname -s)/$ARCH without Docker"
    continue
  fi
  case "$v" in
    2.0.4) commit=$COMMIT_204 platform=386 args= ;;
    *) commit=$COMMIT_210B3 platform=amd64 args="XCFLAGS=-DLUAJIT_ENABLE_GC64" ;;
  esac
  # Natively the default build of each, which is what gmod ships.
  [ "$DOCKER" = 1 ] || args=
  luajit=$(build_vm "$v" "$commit" "$platform" $args)
  case $? in
    0) ;;
    2) echo "SKIP  LuaJIT $v: not built"; continue ;;
    *) fail=1; continue ;;
  esac
  run_on "$v" "$luajit" "$platform"
done

if [ "$GLUA" = 1 ]; then
  # A bundle holds the runtime and the compiled files as strings, so the
  # chunks are checked one by one: the runtime's files, its gmod host, and
  # every NL file of tests/run and tests/gmod that compiles with the
  # profile (helpers too), as `nlc build --profile=portable` prints it.
  echo "== GLua's parser"
  G=$T/glua
  rm -rf "$G"
  mkdir -p "$G/runtime"
  cp "$ROOT"/runtime/*.lua "$ROOT/runtime/gmod/host.lua" "$G/runtime/"
  for dir in "$ROOT/tests/run" "$ROOT/tests/gmod"; do
    decl=
    [ "$dir" = "$ROOT/tests/gmod" ] && decl=decl
    (cd "$dir" && find . -name '*.nl' ! -path './reject/*' ! -path './decl/*') | while read -r f; do
      out=$G/$(basename "$dir")/${f#./}
      mkdir -p "$(dirname "$out")"
      (cd "$dir" && "$NLC" build --emit=lua --profile=portable ${decl:+--decl "$decl"} -o "${out%.nl}.lua" "$f") >/dev/null 2>&1 ||
        rm -f "${out%.nl}.lua"
    done
  done
  "$ROOT/tests/vm/glua_corpus.sh" "$G" "$T/bundles" || fail=1
fi

[ "$fail" = 0 ] && echo "tests/gmod: all passed" || echo "tests/gmod: FAILED"
exit "$fail"
