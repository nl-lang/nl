# nl_fuzz

Differential and robustness fuzzing of `nlc`: generated programs through both backends, and broken programs for panics.

- **Differential.** A seed gives a well-formed NL program whose output the language defines exactly. It runs through `nlc run` under every combination of options that must agree: `--backend=lua` (the oracle) and `--backend=bc`, checked and `--unchecked`, with and without `--no-static-resolution`, and `--jit=on|off|hot`; 24 runs in all, 12 for a checked-only program. Stdout and the exit status must equal the oracle's, and `nlc check` must accept the program.
- **Robustness.** With `--mutate`, broken copies of each program (byte flips, truncation, token deletions, duplications and swaps, stray operators, brackets and keywords, deep nesting) go through `nlc parse`, `nlc check` and, when they compile (about one in twenty), `nlc build` for both backends and `nlc run` under the 24 combinations. Any answer with status 0 or 1 is fine and outputs are not compared, since what a mutant prints may be undefined; a panic, a signal or a hang is not. The oracle run of a mutant gets 5 seconds, and a mutant it cannot finish (a loop whose counter step was deleted) is not run further.

A finding is a disagreement (stdout compared byte for byte), a crash (a signal, an exit status other than 0 and 1, a Rust panic, caught or not, or a stop on one of LuaJIT's own assertions, `LuaJIT ASSERT`, which the debug build of the vendored LuaJIT checks), a timeout, a compile error on a generated program, a failing oracle run, or `nlc` failing to start (`not-started`, so that a broken campaign shows). When the oracle run fails, that is the program's one finding: the other runs are not compared with it.

## Running

```sh
cargo build -p nlc -p nl_fuzz
target/debug/nl-fuzz                          # 200 programs from a seed taken from the clock
target/debug/nl-fuzz --seed 1 --count 5000 --mutate --mutants 2
target/debug/nl-fuzz --print 1234             # the program of seed 1234
target/debug/nl-fuzz --print-mutant 1234 0    # its first mutant
```

| Option | Default | |
|---|---|---|
| `--nlc <path>` | `nlc` next to `nl-fuzz` | The binary under test. A debug build also checks the compiler's `debug_assert`s. |
| `--seed N` | from the clock, printed | Program `i` has the seed `N + i`; `--seed S --count 1` reproduces program `S`. |
| `--count N` | 200 | Programs. |
| `--mutate`, `--mutants N` | off, 4 | Mutants per program; mutant `k` of seed `S` is `S-mk`. |
| `--keep-passing` | off | Also save every program under `<out>/passing/`. |
| `--jobs N` | the number of cores | Parallel workers. |
| `--timeout SECONDS` | 20 | Per `nlc` run. |
| `--out DIR` | `target/fuzz` | Where findings and work files go. |
| `--no-minimize` | | Save findings as found. |

The exit status is 0 when nothing was found and 1 otherwise. Each finding prints one line, `<id>: <kind> [<run>] <detail> -> <dir>`, and is saved under `<out>/findings/<id>/`: the source (`program.nl`, or `mutant.nl` with `original.nl`), `summary.txt` with every finding and the commands that reproduce it, `<run>.stdout` and `<run>.stderr` for every run, and `min.nl` with `min-<run>.*`, the program shrunk while the first finding stays the same (`src/minimize.rs`: delta debugging over top-level statements, then single statements and lines). Only the first two findings of each signature are minimized (a mismatch is known by the configuration that disagreed, anything else by its kind, command and message without digits), and at most 25 per campaign, so that one frequent bug does not keep a campaign busy. Runs follow tests/run/README.md: a fresh process per run, the working directory set to the program's, `NL_PATH` unset.

The smoke test in CI is `crates/nlc/tests/differential.rs`: 40 programs from a fixed seed and one mutant of each, in a few seconds.

```sh
cargo test -p nlc --test differential
```

## The generated language

Programs stay in the part of NL whose output is defined, and print a trace of what they compute (`src/generator/mod.rs` has the rules):

- numbers: integer and float literals in every form, `+ - * / // % **`, the bitwise operators and shifts on integers, `as String`, `as Number`, `as Bool`, `tostring`, comparisons, `Number.floor/ceil/abs/max/min/sqrt`; every expression has a bound on its magnitude, so integers stay exact, divisors and moduli are nonzero literals, and bitwise operands are integers;
- strings: every literal form except raw heredocs, interpolation of numbers, booleans, strings and structs with `To<String>`, `<>`, and the string library (`length`, `len`, indexing by position and range, case mapping, `reverse`, `strip` and its kin, `center`/`ljust`/`rjust`, `sub`/`gsub` with plain strings, `index`, `start_with`/`end_with`, `split`, `chars`, `codepoints`, `as_bytes`, `byteslice` on ASCII, `ord`, `chr`, `format`, `rep`, `valid_encoding`), `"""-` heredocs;
- lists and maps with spreads, `Table.insert`, `Table.concat`, `Table.sort`; lookups with a default, `m.k or 0` and `m?.k ?? 0`, `l[i] or 0` and `l?.[i] ?? 0`; maps are only walked through sorted keys;
- `let`, `const`, typed declarations, assignment, every compound assignment, multiple assignment, `if`/`elseif`/`else`, `unless`, statement modifiers, `if let`, `case` with values, ranges, enum variants and guards, `cond`, `while`/`until`/`loop`/`for` over ranges (with `step`, also negative and fractional), range values, lists, `ipairs`, strings, sorted maps and patterns, `break`/`continue`, `do` blocks;
- functions with defaults, `*rest`, `**opts` and keyed arguments, two return values, early `return`, typed and untyped, command calls, bare calls of function-kind names, closures over locals, parameters and loop variables (factories, closures made in loops), `let f = fn ... end` and the short functions `|a, b| -> e`, `a -> e`, `|| -> e` wherever a one-line `fn` stands, recursion, pipes, `f!`, functions taking blocks (`do |n| ... end`, inline `fn` or short function, `f!` as the block), generic functions;
- structs with typed, untyped and defaulted fields, inheritance, methods (auto-called and explicit), `@field`, `Self`, `To<String>`, operators `+ < <= == len ()`, struct and table patterns in `case` (`{key: x}`, `{v}`, `{v: Number}`, `[a, b]`, `[a, *rest]`, `[a, *] as b`, `Name {field: x}`, `is T`); enums with name, string and number values, `values`, `name`, `is`; interfaces and `x is I`; modules with constants and functions; methods on `Number` and `String`;
- `try`/`catch`/`finally` as statements and values, `raise` of strings and exception structs (with inheritance), `MatchError` and `ConversionError` on purpose, re-raised errors leaving functions, `break`/`continue`/`return` inside `try`; a few programs catch a `TypeError` of a runtime check and are run checked only.

Left out, and why: anything whose output is not defined or not stable (iteration order of `pairs`, table and function addresses, NaN, error messages of Lua errors, `trace`), anything that needs more than stock LuaJIT or a host (regex, workers, `Task.sleep`, events, `include`, `require`, imports, Lua paths, declaration files), raw heredocs (whose text would change with indentation), and Unicode case mapping beyond what the stand-in library of stock LuaJIT does (both backends share it, so it is compared, but full Unicode case mapping is not checked).

**The sign of zero.** `//` never yields `-0`, but other operations can, and a dual-number LuaJIT (ARM64) does not keep it stable: a value is an integer or a double depending on whether the JIT compiled the code, and only a double can be `-0`, so `x * 0` with a negative `x` prints `-0` or `0` on either backend. The generator therefore prints and converts every number as `x + 0`, which makes `-0` `0`; outputs are compared byte for byte.

## Layout

| File | |
|---|---|
| `src/rng.rs` | SplitMix64, so that a seed gives the same program everywhere; no dependency. |
| `src/generator/` | The program generator: `mod.rs` (types, scopes, the program), `expr.rs`, `stmt.rs`, `decl.rs`, `extra.rs`. |
| `src/runner.rs` | Runs `nlc`, the option combinations, what counts as a finding. |
| `src/mutate.rs` | Mutations for robustness fuzzing. |
| `src/minimize.rs` | Shrinks a finding. |
| `src/campaign.rs` | Seeds, threads, saving findings. |
| `src/main.rs` | The `nl-fuzz` command line. |
