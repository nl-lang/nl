# NL

NL is a scripting language for games and mods. It runs on a fork of LuaJIT 2.1
and shares that VM with ordinary Lua code: the same values, tables,
metatables, coroutines and C API. Either language can load and call the other.

```nl
struct Player
  name: String
  health: Number = 100
  armored: Bool = false
end

fn Player.damage(amount: Number)
  @health = Number.max(@health - amount, 0)
  self.die if @health == 0
end

fn Player.die
  print "#{@name} died"
end

on PlayerHit as 'armor' do |target, amount|
  return amount / 2 if target.armored
end

on PlayerHit do |target: Player, amount: Number|
  target.damage amount
end

fn describe(msg)
  case msg
  when {kind: "move", to: dest}
    "moving to #{dest}"
  when Player {name} if name.len > 3
    "a player with a long name"
  when is String
    "a message: #{msg}"
  else
    "something else"
  end
end

let ayla = Player { name: "Ayla" }
Event.run "PlayerHit", ayla, 120     # Ayla died
print describe(ayla)                 # a player with a long name
```

> **Status:** NL is pre-1.0 and under active development. The language, the
> C API and the Rust API can still change. See [Platform support](#platform-support).

## Features

- **Compact syntax for event-driven code.** Lua's foundation, with ideas from
  Ruby, Rust, Elixir, TypeScript and Swift: structs with methods, interfaces,
  enums, operator overloading, pattern matching with `case`, string
  interpolation, blocks, pipes, short functions (`x -> x * 2`), and nil-safe
  `?.`, `??` and `??=`.
- **Optional types that mean something.** A declared type is checked at
  runtime and also tells the compiler what a value is, so it can emit direct
  code instead of dynamic tests. Types that are only inferred never change
  what a line does. The analyzer reports type errors and lints, both in the
  compiler and in the editor.
- **Full Lua compatibility.** The fork runs unmodified Lua source and stock
  LuaJIT 2.1 bytecode with unchanged behavior. NL calls Lua functions,
  libraries and host APIs directly, and Lua code can `require` an `.nl` file.
- **Events, tasks and async.** `on` handlers are shared by NL, Lua and the
  host. `async fn` and `.await` are built on coroutines, and the host drives
  them with one call to `nl_pump` per frame.
- **Workers without data races.** `spawn` runs a function or a block on
  another thread, in a separate VM state. Values are copied across, and
  nothing is shared, so scripts need no locks.
- **Performance on par with Lua.** NL compiles straight to LuaJIT bytecode,
  with five extra opcodes. Generated code is benchmarked against equivalent
  hand-written Lua under both the interpreter and the JIT (see [bench/](bench)).
- **Embeddable from C and Rust.** The C API extends LuaJIT's. The `nl` crate
  extends [mlua](https://github.com/mlua-rs/mlua). An optional sandbox profile
  turns off bytecode loading and FFI, and restricts the global environment
  for untrusted mods.
- **Tooling.** `nlc` checks, builds, runs and formats code. `nl-lsp` is a
  language server with diagnostics, hover, go-to-definition and completion.
  Tracebacks point at NL source lines.

### Workers

```nl
fn count_primes(limit: Number): Number
  let n = 0
  for i in 2..limit
    let prime = true
    for d in 2..Number.floor(i ** 0.5)
      if i % d == 0
        prime = false
        break
      end
    end
    n += 1 if prime
  end
  n
end

async fn report
  let a = spawn count_primes(50_000)    # runs on a worker thread
  let b = spawn count_primes(100_000)   # and this one on another
  print "#{a.await} and #{b.await} primes"
end

report() unless Worker.parent
```

## How it works

```
  .lua source ──► LuaJIT's parser (unchanged) ──┐
                                                ├─► bytecode ─► VM (interpreter + JIT)
  .nl source ───► NL compiler (Rust, libnlc) ───┘        ▲
                    lexer → parser → resolver →          │
                    analyzer → core IR → bytecode    NL runtime library
                                                     (structs, patterns, exceptions,
                                                      tasks, regex, events, workers)
```

- **Compiler**: written in Rust and linked into the VM as a static library. It
  takes source text and a callback that finds imported files, and returns
  bytecode and diagnostics. It never calls into Lua. The same compiler ships as
  the `nlc` command-line tool.
- **VM**: a [fork of LuaJIT 2.1](https://github.com/nl-lang/LuaJIT). It adds
  five opcodes, the metamethod events Lua 5.1 lacks, and a C event library. It
  also adds the loader, the transfer encoder, and the threads behind workers.
  All of these are additive: no stock opcode, value representation or C API
  function changes meaning.
- **Runtime library**: Lua code embedded in the VM that provides what compiled
  NL relies on, plus the standard library. Regex (the Rust `regex` crate) and
  UTF-8 string operations are linked in with the compiler.

## Getting started

### Requirements

- Rust 1.85 or newer
- A C compiler and GNU make (to build the VM fork)
- Git, with submodules

```sh
git clone --recursive https://github.com/nl-lang/nl.git
cd nl
```

### Trying the language

`nlc` runs programs on stock LuaJIT, which it builds itself, so this needs no
VM build:

```sh
cargo build --release -p nlc
target/release/nlc run example.nl
```

| Command | What it does |
|---|---|
| `nlc run [--backend=lua\|bc] [--jit=on\|off\|hot] <file>` | Compile and run a file |
| `nlc check [--decl <path>] <file>...` | Report errors, warnings and lints |
| `nlc build --emit=lua\|bc [-o path] <file>` | Print the generated Lua, or write a bytecode dump |
| `nlc fmt [--check] <file>...` | Format files in place |
| `nlc parse <file>...` | Print the syntax tree |
| `nlc bundle [--profile=gmod-client] [-o path] [--map path] <entry.nl>...` | Write the runtime and a program as Lua for stock LuaJIT |
| `nlc runtime [--profile=gmod-client] [-o path]` | Write the runtime alone, for bundles made with `--runtime-include` |
| `nlc remap <map.json>...` | Map positions in an error or a log back to source files |
| `nlc minify [--glua] <file.lua>` | Minify Lua or GLua, keeping line numbers |

`nlc --help` lists every option.

### Garry's Mod

NL compiles to client-side Lua for the stock game, whose clients run LuaJIT
2.0.4 (main branch) or 2.1.0-beta3 (x86-64 branch). From the addon's `lua/`
folder:

```sh
nlc bundle --profile=gmod-client -o autorun/client/myaddon.lua \
           --map ../myaddon.nlmap.json myaddon/main.nl
```

- **Checks.** The `gmod-client` profile rejects anything the stock client
  doesn't have, at compile time: what Garry's Mod removes from Lua (`io`,
  `loadstring`, most of `os`, …), LuaJIT 2.1-only functions, and gmod
  functions, methods and values that exist only on the server or in the
  menu. Garry's Mod's API declarations (`decl/glua.d.nl`) are built in.
  `nlc check --profile=gmod-client` reports the same errors.
- **`if SERVER`.** Code under `if SERVER`, `unless CLIENT`, `x if SERVER` and
  other conditions decided by `SERVER` and `CLIENT` is not checked and is
  left out of the output. A shared file can keep its server half, and that
  code never ships to players.
- **Output.** The runtime and the program are minified (line breaks stay, so
  line numbers still match) and checked to load on LuaJIT. Garry's Mod sends
  a client file only if it is at most 64 KiB compressed, so a bundle that
  doesn't fit is split into `myaddon.lua`, `myaddon_2.lua`, … These parts
  load in any order, and the last one to arrive runs the program.
- **One runtime.** Every bundle carries NL's runtime (about 30 KiB
  compressed) unless it is made with `--runtime-include nl/runtime.lua`:
  then it includes that file when no runtime is loaded yet, and
  `nlc runtime --profile=gmod-client -o nl/runtime.lua` writes the file.
  Clients download the runtime once, in one file every bundle shares, and
  keep it in their cache until it changes.
- **Included parts.** With `--include-parts` the first file of a bundle in
  parts includes the others by their Lua paths (the `-o` path, from the
  addon's `lua/` folder), so the first file alone runs the bundle: an
  `include` of it, or an entity's `cl_init.lua`, wherever the parts are.
- **Autorefresh.** A bundle that runs again (gmod sends clients the new
  file when it changes on the server) runs again the files in it whose
  source changed, and the files that import them; the runtime's
  `rt.forget` does it, and a host that reloads changed files can call it
  too.
- **Sourcemap.** `--map` writes a JSON map from each chunk's lines (the
  `short_src` and line LuaJIT reports) to NL and runtime source lines, for a
  host that catches errors. `nlc remap` applies it to any text. Keep the map
  out of `lua/`, since only `.lua` files belong there.
- **From a host.** The packager, the minifier and sourcemaps are also a C
  API in the compiler library (`libnl_capi.a`), declared in
  [`crates/nl_capi/include/nlbundle.h`](crates/nl_capi/include/nlbundle.h).
  It is pure: files come through the host's loader, and the host's VM checks
  that chunks load. A host like a replacement `lua_shared` can package NL
  for clients in-process and map the positions of Lua errors back to NL
  source with the bundle's sourcemap. `tests/vm/minify_corpus.sh` checks the
  GLua minifier on Garry's Mod's own Lua: each file must compile to the same
  bytecode, debug info included.

### Building the VM

The full language, including the five extra opcodes, workers and the sandbox,
needs the fork. Build the compiler library, then the VM against it:

```sh
cargo build --release -p nl_capi
make -C LuaJIT/src NL_RUNTIME_DIR="$PWD/runtime" \
     NL_LIBNLC="$PWD/target/release/libnl_capi.a"
```

This produces `LuaJIT/src/luajit` and `LuaJIT/src/libluajit.a`. On macOS,
set `MACOSX_DEPLOYMENT_TARGET` (for example `11.0`) first, as for stock
LuaJIT. On Linux, also pass the system libraries the Rust library needs:
`NL_LIBNLC_LIBS="-lgcc_s -lutil -lrt -lpthread -lm -ldl"`.

A host program links both libraries (on Linux, followed by the same system
libraries):

```sh
cc -ILuaJIT/src host.c LuaJIT/src/libluajit.a target/release/libnl_capi.a -lm
```

### Editor support

```sh
cargo build --release -p nl_lsp
```

The `nl-lsp` binary speaks the Language Server Protocol over stdio, so any
generic LSP client works with it. In Neovim, for example:

```lua
vim.lsp.start({ name = 'nl', cmd = { 'nl-lsp' } })
```

## Embedding

### From C

`luaL_openlibs` opens NL along with Lua's libraries. Loading and calling work
like their `luaL_*` counterparts:

```c
#include <stdio.h>

#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
#include "nl.h"

static int open_libs(lua_State *L) {
  luaL_openlibs(L);                          /* Lua's libraries, then NL's */
  return 0;
}

static void report(lua_State *L) {           /* pops a message or an Exception */
  fprintf(stderr, "%s\n", nlL_tolstring(L, -1, NULL));
  lua_pop(L, 2);
}

int main(void) {
  lua_State *L = luaL_newstate();
  if (lua_cpcall(L, open_libs, NULL) != 0) return 1;
  nl_addpath(L, "scripts");                  /* where imports are found */

  if (nl_loadfile(L, "scripts/main.nl") != 0 || nl_pcall(L, 0, 0) != 0) {
    report(L);
    return 1;
  }
  while (game_running()) {
    lua_pushnumber(L, frame_time());
    if (nl_prunevent(L, "Game.Tick", 1, 0) != 0) report(L);  /* every `on Game.Tick` */
    nl_pump(L);                              /* resume tasks, deliver results */
  }
  lua_close(L);
  return 0;
}
```

`LuaJIT/src/nl.h` declares the whole API: host types, marking methods, events,
tasks, timers, workers, options and the sandbox.

### From Rust

The `nl` crate builds on mlua. The VM comes in through
[luajit-src-rs](https://github.com/nl-lang/luajit-src-rs), a patch of the
`luajit-src` crate that mlua builds LuaJIT with: it builds the fork, with
NL's runtime.

```toml
[dependencies]
nl = { git = "https://github.com/nl-lang/nl", features = ["async"] }

[patch.crates-io]
luajit-src = { git = "https://github.com/nl-lang/luajit-src-rs" }
```

The two must match: to pin `nl` to a revision, pin `luajit-src` to the commit
that revision's `luajit-src-rs` submodule names.

```rust
use nl::prelude::*;

fn main() -> LuaResult<()> {
    let lua = nl::new()?;
    lua.load_nl_file("main.nl")?.try_call::<()>(())?;
    while game_running() {
        lua.run_event::<()>("Game.Tick", frame_time())?;
        lua.pump();
    }
    Ok(())
}
```

`tests/vm/nl_example` is a complete program, with a host type, async host
functions and error reporting.

## Repository layout

| Path | Contents |
|---|---|
| `crates/nl_syntax` | Lexer, error-recovering parser and lossless syntax tree; the formatter |
| `crates/nl_sema` | Name resolution, types, the analyzer and lints |
| `crates/nl_ir` | Core IR and lowering from the syntax tree |
| `crates/nl_luagen` | IR to Lua source: a debugging aid and the test oracle |
| `crates/nl_codegen` | IR to LuaJIT bytecode, the dump writer and a bytecode verifier |
| `crates/nl_capi` | The C ABI static library (`libnlc`) that the VM links |
| `crates/nl_regex`, `crates/nl_text` | Regex and UTF-8 string libraries |
| `crates/nlc` | The command-line tool |
| `crates/nl_lsp` | The language server |
| `crates/nl_sys`, `crates/nl` | Raw and safe Rust bindings, on mlua |
| `crates/nl_fuzz` | A differential fuzzer that runs generated programs through both backends |
| `runtime/` | The runtime and standard library (Lua, embedded in the VM) |
| `decl/` | Declaration files (`.d.nl`) for the Lua and LuaJIT standard libraries |
| `LuaJIT/` | The VM fork (submodule) |
| `luajit-src-rs/` | The `luajit-src` crate that builds the fork for Rust hosts (submodule) |
| `tests/` | Parser snapshots, conformance programs, Lua compatibility suites, VM tests |
| `bench/` | Benchmarks of NL against equivalent Lua |

## Testing

```sh
cargo test --workspace     # compiler, analyzer, formatter, LSP; conformance on stock LuaJIT
tests/vm/run.sh            # the fork: C API, sandbox, workers, conformance with the JIT on, off and hot,
                           # AddressSanitizer and ThreadSanitizer builds, the Rust host crates
tests/lua-compat/run.sh    # the Lua test suites on the fork, compared with stock LuaJIT
bench/run.sh               # benchmarks
```

The conformance programs in `tests/run` run on every backend and VM mode, and
each one must print exactly its expected output. `nl-fuzz` generates random
programs and compares the Lua and bytecode backends:

```sh
cargo run --release -p nl_fuzz -- --seed 1 --count 1000 --mutate
```

## Platform support

| Platform | Status |
|---|---|
| macOS, arm64 | Developed and tested here |
| Linux, x64 and arm64 | Targeted; the CI workflow runs the full suite there |
| Windows | The worker threads have a Windows code path, but it has not been built yet |

The VM targets 64-bit builds (GC64), both with the JIT and interpreter-only,
for platforms that forbid JIT compilation.

## License

NL is released under the [MIT License](LICENSE). The LuaJIT fork keeps LuaJIT's
own MIT license, Copyright © Mike Pall.
