# Conformance programs

Small NL programs, one feature each, with the exact output they must print. They exercise the parser, the analyzer, both backends (Lua source and bytecode) and the runtime, including workers. Each one says what it checks in its first line.

## Layout

| Path | What it is |
|---|---|
| `<area>_<feature>.nl` | A program to run. |
| `<area>_<feature>.out` | Its exact expected stdout, byte for byte, ending in a newline. |
| `reject/<name>.nl` | A program the compiler must reject for a non-syntactic reason. No `.out`. |
| `import/`, `include/`, `lua/`, `worker/` | Helper files used by other programs. Never run on their own. |

A `.nl` file is an entry program exactly when a `.out` file of the same name sits beside it.

Area prefixes name the part of the language: `lex_` (lexical structure), `var_` (declarations and scope), `type_` (types and runtime checks), `expr_` (expressions), `fn_` and `call_` (functions and calls), `ctrl_` and `pattern_` (control flow and patterns), `err_` (exceptions), `struct_`, `iface_`, `conv_`, `enum_`, `op_`, `prim_` (structs, interfaces, conversions, enums, operators, primitive methods), `module_`, `import_` (modules and files), `event_` (events), `lua_` (Lua interop and Lua's libraries), `task_` (tasks), `regex_` (regular expressions), `string_` (the string library), `table_` (the table library), `time_` (`Time` and `Duration`), `worker_` (`spawn` and workers).

## Running a program

- Run `nlc run <file>.nl` with the working directory set to `tests/run/`. `include(...)` and `require(...)` take paths relative to the working directory (`include("include/counter.lua")`, `require("lua.helper")`); `import` paths resolve relative to the importing file and do not depend on it.
- Run it again with `nlc run --no-static-resolution <file>.nl`: the analyzer's proven checks and static resolution from declared types are then left out of the generated code, and the output must be the same. The runner in `crates/nlc/tests/conformance.rs` does both.
- Run it again with `nlc run --localize=none` and `--localize=all`: which Lua paths a file reads once into hidden locals (the default is `std`) must not change the output either. The runner does this on the Lua backend and on the bytecode backend with the JIT forced.
- Run with `NL_PATH` unset, so that the only search-path entries are those a program adds with `add_path`.
- Compare stdout with the `.out` file exactly. Ignore stderr. The exit status must be 0.
- `nlc run` pumps after the main chunk until no task is pending. Every task a program creates finishes, so a run that does not end is a failure.
- Each program runs in a fresh process: several programs define the same primitive methods (`Number.is_cool`, `String.*`, …), which is allowed only once per Lua state.
- Programs print through `print`, so numbers appear as Lua's `tostring` gives them (`%.14g`: `3`, `3.5`, `5e+20`) and several arguments are separated by a tab (`call_command.out` has one such line).

## Which programs need what

| Files | Needs |
|---|---|
| `regex_*` | the regex library of the fork; on stock LuaJIT (`nlc run`, the oracle) `Regex.new` raises, so they are excluded there |
| `string_case_unicode.nl`, `string_case_styles.nl`, `string_casecmp.nl`, `string_lua_methods.nl` | Unicode case tables: the fork's string library has them, and the Lua stand-in used on stock LuaJIT reads the same mappings from its host (`nlc run` gives them; a portable bundle carries them unless it is built without) |
| `worker_*` | the worker runtime of the fork, where they run with 0, 1 and 8 pool workers (`tests/vm/conformance.sh`); on stock LuaJIT the same-state stand-in of `spawn` runs all of them but two: `worker_dedicated` (it needs `Worker.new`) and `worker_spawn_block` (it reads a file-level variable that each worker has a copy of) are excluded there |
| `reject/type_*` | the analyzer: these are type errors, caught in the compiler, so on every target |
| `reject/regex_*` | compile-time validation of literal patterns, in the compiler, so on every target |
| `reject/worker_*` | the resolver's `spawn` checks |

Everything else must pass through both backends, with the JIT on, off and forced, and with static resolution from declared types on and off. The worker programs must print the same output with 0, 1 and N pool workers.

Every file, including the helpers and the rejected programs, must parse. `nlc check` on a program to run reports no error; warnings and lints are expected in some files (shadowing in pattern tests, unused bindings, the use of a `global`, a file-level variable read in a `spawn do` block, a Lua path that names the function it is in). `crates/nlc/tests/check.rs` runs `nlc check` on every entry program and holds the table of the diagnostic codes each one is expected to produce; a new program that is expected to warn is added there.

## Rejected programs

Each file in `reject/` contains exactly one error, which is not a syntax error: the file parses, and the resolver or the analyzer rejects it. The line before the offending line is a comment of this form:

```nl
# expect-error: <what is wrong and why>
```

A runner checks that compiling the file fails and that a diagnostic points at the line after that comment. The text after `expect-error:` is for people; diagnostics are not matched against it.

## Writing more programs

- One feature per file, roughly 15 to 60 lines, deterministic, printing what it computes. A reader must be able to check the `.out` by hand.
- Never print a table or a function directly: their addresses change between runs. Iterate a map through sorted keys.
- An error that a feature raises is caught with `try`/`catch` and printed by type name (`typeof(err)` or a typed `catch`), not by message, unless the message is what the program checks.
- Values that must fail a runtime check come from a variable declared `any`, so that the analyzer cannot reject the program at compile time.
- Code that needs the regex library goes in `regex_` files, and code that needs workers in `worker_` files. Avoid `Task.sleep` outside worker files: in the main state it needs a host timer.
- A `time_` program builds its times with `Time.at` and reads them with `utc: true`: what it prints must not depend on the wall clock or the machine's time zone. Local time and `Time.now` are checked through relations that hold everywhere (`t.hour == (t + t.utc_offset).hour(utc: true)`).
- A worker program keeps its top level free of side effects that a worker would repeat when it loads the file: put the program in a function and call it with `main() unless Worker.parent`, or call only functions imported from a helper file, which is then the only file a worker loads.
