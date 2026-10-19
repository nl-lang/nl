# Benchmarks: NL against hand-written Lua

NL against equivalent hand-written Lua, in the interpreter and with the JIT,
with parity within a few percent as the target, and measurements for deciding
whether floor division, bitwise operators, string conversion or `try` deserve
their own opcodes (and whether auto-call on member reads slows field access).
This directory holds the programs, the runner and the record of a run
([RESULTS.md](RESULTS.md)). The last section gives the measurements and a
recommendation for each.

## Running

```sh
bench/run.sh                 # every pair; writes bench/RESULTS.md
bench/run.sh bitops try_ok   # some pairs; writes target/vm/bench-results.md
```

The runner:

1. builds `libnl_capi.a` (release), the fork twice with the real compiler and
   runtime, as `tests/vm/conformance.sh` does (`target/vm/bench-jit`, and
   `target/vm/bench-nojit` with `-DLUAJIT_DISABLE_JIT`, the interpreter-only
   build; `-joff` is not the same thing), and the host
   [nlbench.c](nlbench.c) against each (`target/vm/nlbench-jit`,
   `target/vm/nlbench-nojit`);
2. waits until no `cargo`, `rustc`, `cc`, `clang` or `make` process has been
   seen for a minute (`pgrep`, every 30 s), and checks again before each pair;
3. for each pair and each build, runs the Lua program, the NL program and the
   NL program compiled unchecked, and refuses to time the pair unless the
   three print the same checksum;
4. times the three `NLB_RUNS` times (default 5), interleaved and in rotating
   order, as the wall-clock time of the whole process, and keeps the minimum
   and the median; when a median is more than 10% above its minimum, the
   pair is measured again (`NLB_RETRIES`, default 2 more times) and the
   attempt with the smallest spread is kept;
5. prints a Markdown table and writes it to `RESULTS.md`, with the machine,
   the date, the commits and the load averages at the top.

`NLB_NOWAIT=1` skips the wait for a quiet machine, for trying things out.

**The host.** `nlbench` opens a state as `tests/vm/conform/nlrun.c` does
(`luaL_openlibs`, which opens the NL runtime), loads the file with
`luaL_loadfile` (the extension picks the language) and runs it with
`nl_pcall`. Both sides of a pair run in the same host, so they differ only in
the file. `--unchecked` sets `NL_OPT_CHECKS` to 0 before loading, which is
the unchecked build (`nlc build --unchecked`). `--list` prints
the bytecode of the loaded chunk instead of running it.

**Iterations are pinned.** Every program repeats a fixed workload
`NLB_SCALE` times (the environment variable, default 1). The interpreter
runs at scale 1; the JIT build runs at a scale per pair, fixed in `run.sh`
(`jit_scale`), so that the JIT runs last long enough to time. Ratios are
taken between the two sides of one pair in one build; times across builds
are not comparable where the scales differ.

**Startup.** The time of a process includes creating the state, opening the
runtime and, for NL, compiling the file: about 3 to 7 ms on the machine of
RESULTS.md, measured by the `empty` pair. The *net* columns subtract it,
which matters for the short JIT runs.

**Looking at the code.** The bytecode NL compiles to on the fork (level 1),
and the traces:

```sh
export LUA_PATH="target/vm/bench-jit/src/?.lua;;"
target/vm/nlbench-jit --list bench/bitops.nl              # checked
target/vm/nlbench-jit --unchecked --list bench/bitops.nl
NLB_SCALE=1 target/vm/bench-jit/src/luajit -jv bench/bitops.nl
```

(`nlc build --emit=bc` emits level 0, the dump for stock LuaJIT, without the
fork's opcodes; it is not what runs here.)

## The pairs

Each NL program has a Lua twin written as a Lua programmer would write it:
locals for library functions (`local floor = math.floor`), `bit.*`, `..`,
`string.format`, a metatable class with `:` calls, `pcall`. The comment at
the top of each file says what it measures.

| pair | measures |
|---|---|
| `empty` | startup: state, runtime, compiling the file, one `print` |
| `num_float` | float `+ - *`, comparisons, nested `while` (Mandelbrot) |
| `num_int` | integer-valued `+ - *`, comparisons in a nested numeric `for` |
| `floordiv` | `//` and `%` in a loop (digit sums) against `floor(a / b)` and `%` |
| `bitops` | `^ << >>> & \|` (xorshift, popcount) against `bit.*` |
| `bitops_typed` | the same with the variables declared `Number` (the bit opcodes on the fork) |
| `str_build` | interpolation of numbers, `<>`, `tostring`, `Table.concat` against `..`, `tostring`, `table.concat` |
| `str_format` | `"…".format(…)` through a string receiver against `string.format` |
| `str_methods` | NL `String` methods (`upcase`, `reverse`, `index`, `byteslice`, `start_with`, `length`, `strip`) against `string.*` |
| `list_index` | reads and writes by index, `.len` (a sieve and prefix sums) |
| `fields` | field reads and writes on struct instances, receiver untyped (`TGETS` + `ISMETH`) against plain tables |
| `fields_typed` | the same, receiver declared `p: Particle` (plain `TGETS`; checked: one type test per particle) |
| `methods` | `v.norm2` (auto-call), `acc.add(w)`, `acc.dot(v)` (member calls, `MSELF`), untyped receivers, against `obj:method()` |
| `methods_typed` | the same with declared struct types and typed parameters (static calls; checked: parameter and return checks) |
| `closures` | a closure made per iteration and called; a block passed to a `Function` parameter |
| `blocks` | an `each`-style helper called with a block that captures the loop variable, a block that captures an accumulator, a block inside a block (see "Blocks in loops") |
| `case_match` | `case` with values, a range, `is String`, table patterns that bind, a guard, against if-chains |
| `try_ok` | `try`/`catch` around a call that does not raise, against `pcall` |
| `try_raise` | `raise` of an exception struct caught by type every iteration, against `error(t)` and `pcall` |
| `struct_new` | `Vec { x: …, y: … }` per iteration against `setmetatable({…}, Vec)` |
| `lua_path` | `:math.floor` and `:math.sin` written at each use in a hot loop, against `local floor, sin = math.floor, math.sin` (see "Localized library paths") |
| `struct_import` | the same for structs declared in another file (`struct_import_lib.nl`): by an imported name and through the export table, with a parent, a default and an optional field left out, and a parameter of the imported type |
| `raytrace` | mixed: a ray tracer with `Vec3` and `Sphere` structs and typed methods |
| `interp` | mixed: a stack-machine interpreter, `case` on opcode strings, lists |

## What the run shows

Ratios below are NL/Lua from [RESULTS.md](RESULTS.md) (minima; *checked* is
the default build, *unchecked* is `NL_OPT_CHECKS=0`), interpreter first, then
JIT.

**At parity** (within a few percent, or NL ahead): `num_float`, `num_int`,
`list_index`, `str_build` and `str_format` with the JIT, `interp` with the
JIT, `floordiv` with the JIT, `methods` and `methods_typed` with the JIT
(NL about 10% ahead there: the static and `MSELF` calls trace as well as
Lua's `:` calls, and NL's struct methods are found one `__index` step away
like Lua's). Plain arithmetic, comparisons, numeric loops and list indexing
compile to the same bytecode as the Lua programs.

**Slower, and why** (each traced to its hot path in the sections below and
in "Other costs"):

| pair | checked | unchecked | where the time goes |
|---|---|---|---|
| `bitops` | 4.26 / 133 | same | a Lua wrapper per operator; with the JIT, a trace cliff (tail calls in the wrapper count toward the loop-unroll limit) |
| `floordiv` | 2.23 / 1.01 | same | a Lua wrapper (and a second Lua function) per `//` |
| `str_build`, `str_format` | about 1.15 / 1.0 | same | a `tostring` call per interpolated number; `"…".format` looked up through the string receiver |
| `fields` | 1.17 / 1.58 | same | struct instances built by `rt.new` (not the auto-call test) |
| `fields_typed` | 4.02 / 5.81 | 1.20 / 1.54 | the annotation check, a chain of runtime calls per particle |
| `methods`, `methods_typed` | 2.47, 8.97 / about 0.9 | 1.10, 1.01 / 0.89 | `rt.nokw` at every function entry; parameter and return checks |
| `closures` | 6.12 / 1.52 | about 1.0 / 1.0 | `rt.nokw` and the `Function` check per call; `FNEW` stops both sides' traces |
| `case_match` | 6.81 / 1.27 | 4.82 / 1.34 | a Range allocated per `when 3..9`, `rt.istable` per table arm, one-shot `LOOP` blocks |
| `try_ok` | 16.9 / 98 | 13.2 / same | two closures and two tables per `try`; no trace |
| `try_raise` | 2.02 / 1.55 | same | unwinding dominates both sides; NL adds a trace walk, an `rt.new` and an `rt.isa` per catch |
| `struct_new` | 19.6 / 5.86 | 6.64 / 5.57 | `rt.new`: two extra tables, loops and per-field `rt.isa`; its loops break traces |
| `str_methods` | 4.56 / 3.21 | same | NL's UTF-8 string library (encoding check, character lists) against Lua's byte functions |
| `raytrace` | 16.2 / 32.3 | 3.30 / 17.3 | all of the above at once: a struct construction per vector operation, checks, `rt.nokw`, `rt.ret` |

## Opcode decisions

There are four candidates for opcodes and one risk. For each: what the
fork runs today, what the Lua twin runs, the measured ratio, and a
recommendation.

### Floor division `//`

- **Today.** `a // b` is `KINTR idiv; MOV; MOV; CALL` (see `--list`), a call
  of `rt.idiv` (runtime/nl_40_ops.lua), which is `numeric(f, "__idiv")`: a
  Lua closure that tests `type(a)` and `type(b)` and tail-calls `f`, itself a
  Lua function `return floor(a / b)` that tail-calls `math.floor`. Two Lua
  calls and a fast-function call per `//`. This holds when the analyzer knows
  both operands are numbers: `binary_op` in crates/nl_ir/src/lower/operator.rs
  uses the analyzer's hint only for comparisons.
- **Lua.** `floor(m / 10)` with `local floor = math.floor`: `DIVVN` and one
  fast-function call (`math.floor` is assembler in the interpreter, one
  machine instruction in a trace).
- **Measured.** `floordiv` (one `//` and one `%` per inner iteration):
  interpreter 2.23, JIT 1.01. The JIT inlines both Lua
  functions, so traces end up as Lua's.
- **Hazard.** Each `//` makes two tail calls, and the recorder counts tail
  calls toward `loopunroll` (15): a loop body with about eight `//` never
  gets a trace, as `bitops` shows for the bitwise operators.
- **Recommendation: a cheaper lowering, no opcode.** (1) When both operands
  are proven numbers, emit `DIVVV` and a call of the raw `math.floor` through
  `KINTR` (a new intrinsic slot holding the fast function), which is the
  Lua twin's bytecode: parity in the interpreter too. (2) For other
  operands, make `rt.idiv` one function with no tail call
  (`if both numbers then local r = floor(a / b) return r end`). An opcode
  (`IDIVVV`/`VN`/`NV`, both interpreters, the recorder, the `__idiv`
  fallback) would only beat (1) by the cost of one fast-function call in the
  interpreter; the JIT gains nothing.
- **Applied.** (2) as described. For (1), operands declared `Number`
  lower to `local q = rt.nfloor(a / b)` and `if q == 0 then q = 0 end`
  (crates/nl_ir/src/lower/operator.rs): `rt.nfloor` is `math.floor` as
  the runtime found it, a new intrinsic at the end of `NLC_INTRINSICS`, so
  a script replacing `math.floor` changes nothing.
  `math.floor(a / b) + 0` was 15% slower in the interpreter. With
  `nlc run` and declared types (`let m: Number`): interpreter 0.81 s →
  0.54 s, JIT unchanged; the Lua twin is 0.47 s. The fork's
  floordiv.nl declares nothing, so only (2) applies to it.

### Bitwise operators `& | ^ << >> >>> ~`

- **Today.** Each operator is `KINTR` + `CALL` of `rt.band`, `rt.bor`, …, the
  same `numeric` wrapper as `//` around `bit.*`: one Lua call that tests both
  types and tail-calls the `bit` fast function.
- **Lua.** `bxor(x, lshift(x, 13))` with locals for `bit.*`: fast-function
  calls only.
- **Measured.** `bitops` (18 operators per iteration): interpreter
  4.26, JIT 133 (net 141). The JIT figure is a
  cliff, not a slow path: `-jv` shows the loop aborting with "loop unroll
  limit reached" inside the wrapper, then blacklisted, so the whole loop runs
  in the interpreter (the JIT build's time at scale 4 is 4 × its interpreter
  time). The cause is `return f(a, b)` in the wrapper: `lj_record_tailcall`
  counts every tail call toward `loopunroll`. Confirmed twice: the NL
  program runs at Lua's speed with `-Oloopunroll=60` (3.55 s → 0.02 s at
  scale 8), and with the wrapper changed to `local r = f(a, b) return r` in a
  scratch build of the runtime (3.50 s → 0.033 s at scale 2, Lua 0.028 s).
  That change costs about 10% in the interpreter (one more instruction per
  call): 1.79 s → 2.02 s.
- **Recommendation: fix the runtime now, then a VM fallback, no opcode.**
  (1) Remove the tail calls from `numeric` (and from any intrinsic that
  generated code calls in hot loops); this alone takes the JIT from 120× to
  parity. (2) As for `//`, call the raw `bit.*` fast function through `KINTR`
  when the operands are proven numbers: the Lua twin's bytecode, parity in
  the interpreter. (3) For untyped operands, make the fork's `bit.*` fall
  back to the NL metamethods (`__band`, …) instead of raising, which the
  language already specifies but `lib_bit.c` does not do; then `rt.band` *is*
  `bit.band` and the wrapper disappears everywhere (the fallback must refuse
  string coercion when the caller has `PROTO_NL`). That is C in the slow
  path only. Opcodes would mean seven operators times their constant
  variants in two interpreters and the recorder, to win one fast-function
  call per operation in the interpreter.
- **Applied.** (1) for `numeric` and `rt.bnot`; (2) as a call of
  `rt.nband` and its siblings (the `bit` functions as the runtime found
  them, intrinsics at the end of `NLC_INTRINSICS`) when both operands are
  declared `Number` (not `~`, whose operand the analyzer does not
  record). Measured on stock LuaJIT, so without `KINTR`. With `nlc run`: bitops.nl JIT
  at scale 4, 8.8 s → 0.058 s (Lua 0.055 s); interpreter 2.2 s → 2.5 s
  (the expected cost of the non-tail call), and with declared types
  2.2 s → 0.60 s (Lua 0.41 s).
- **Then the opcodes after all, for proven numbers.** Upstream LuaJIT 2.1
  backported the 3.0 bit operators (fork base `a2ce8114`): `BNOT BAND BOR
  BXOR BSHL BSHR BSAR` in both interpreters and the recorder (`IR_BAND`…,
  the same IR as `bit.*`), on numbers exactly `bit.*` (`lj_num2bit`, the
  count taken modulo 32), so the opcode cost above is gone. At level 1 the
  bytecode backend now emits them for `rt.nband` and its siblings and for
  `~` on a proven number (the analyzer records it now; `rt.nbnot`
  elsewhere): one instruction, operands in registers (no constant forms),
  two constants folded (crates/nl_codegen, emit/expr.rs). The prototypes
  do not get `PROTO_BITOP`, which the loader keeps in the bit the trace
  recorder reads as "closures are polymorphic" (crates/nl_codegen,
  proto.rs). Untyped operands keep `rt.band` and the events. `bitops_typed`
  (the program of `bitops` with `let x: Number`…), 3 runs, minima, before →
  after: interpreter 0.414 s → 0.170 s against Lua's 0.40 s (1.03 →
  **0.42**), JIT at scale 4 1.02 → 1.02 (one trace, the IR of `bit.*`
  before and after; at scale 40 both within the noise of the Lua twin,
  about 0.5 s). `bitops` itself declares nothing and does not change (4.97
  / 1.03).

### String conversion (interpolation, `tostring`, `as String`)

- **Today.** Each interpolated value that the analyzer does not know to be a
  string is wrapped in `rt.tostring(v)` (`KINTR 62` + `CALL`), which is
  Lua's `tostring` itself, then joined with `CAT`. `"…".format(…)` reads
  `format` through the string receiver (`TGETS` on a string, resolved by the
  fork to NL's `String` table) and calls it with `MSELF`/`CALLM`.
- **Lua.** `"item " .. i .. ": "` lets `CAT` convert the number; no call.
  `format(…)` through a local.
- **Measured.** `str_build`: interpreter 1.13, JIT 1.03.
  `str_format`: interpreter 1.14, JIT 1.01. Allocation and the
  string functions dominate both sides.
- **Recommendation: a cheaper lowering for numbers, no opcode.** When the
  analyzer proves a value is a number, skip `rt.tostring`: `CAT` converts
  numbers with the same `%.14g` as `tostring`. Other values need the call
  (`__tostring`, `To<String>`), and `tostring` is already a fast function.
  The remaining 10–15% in the interpreter is not worth an opcode.
- **Not applied.** `rt.tostring` is Lua's `tostring`, so emitting
  `tostring(x)` instead would call the same function (str_format.nl did
  not move). `x as String` on a `Number` cannot become `tostring(x)`: a
  `To<String>` for `Number` may be declared in another file, and `rt.as`
  runs it.

### `try`

- **Today.** crates/nl_ir/src/lower/error.rs lowers `try` to
  `rt.try(function() … end, function(e) … end, finally)`: two `FNEW` per
  execution (plus `UCLO` when the body captures loop locals), a call of
  `rt.try` (runtime/nl_50_error.lua), which runs `xpcall(body, rt.handler)`
  and packs its results into a table (`pack(...)`, a vararg call and a
  `TNEW`), and `rt.unpack` of that table. `FNEW` and `UCLO` are not compiled
  by the recorder (NYI in 2.1), so a loop with a `try` never gets a trace.
  When an error is caught, `rt.handler` also builds the exception's `trace`
  with `debug.getinfo` (up to 64 levels) and wraps foreign errors with
  `rt.new`.
- **Lua.** `local ok, v = pcall(parse_digit, i % 10)`: no allocation; the
  recorder compiles `pcall` and `xpcall`.
- **Measured.** `try_ok` (no error): interpreter 16.9 (unchecked
  13.2), JIT 98 (net 105). `try_raise` (error every
  iteration): interpreter 2.02, JIT 1.55; unwinding (about
  5.6 µs per error on this machine, for both sides) hides most of the
  difference.
- **What a cheaper lowering would give.** Two Lua programs written to
  measure it (same loop as `try_ok`): today's lowering written out in Lua
  (closures, `pack`) runs 1.31 s interpreted and 1.15 s with the JIT, like
  the NL program; a lambda-lifted lowering (the body is a function made once,
  the captured `i` passed as an argument, the `"n"` status protocol kept,
  `xpcall(body, handler, i)` called directly) runs 0.167 s interpreted and
  0.015 s with the JIT, against `pcall` at 0.100 s and 0.015 s. That is
  parity with the JIT and 1.7× in the interpreter, from 17× and 90×.
- **Recommendation: lambda-lift the regions, no opcode.** In the compiler:
  make the body, catch and finally functions once (where the enclosing
  function's prototype is created), pass the locals they read as arguments,
  and return the locals they assign through the status protocol (or keep
  today's closure for the rare region that assigns an outer local and also
  escapes); in `rt.try`, return `xpcall`'s results directly instead of
  packing them (`select` on the status), so a successful `try` allocates
  nothing. For the error path, consider building `trace` lazily (store the
  coroutine and level, or `debug.traceback` only when `trace` is read).
  A protected-region opcode would need an inline frame kind in both
  interpreters and, harder, in the recorder (pcall frames, snapshots and
  unwinding); its gain over the lifted lowering is the remaining 1.7× in the
  interpreter, nothing with the JIT. This was "the likeliest" opcode in the
  plan; the measurements say the closure, not the call, is the cost.
- **Landed (after the run of RESULTS.md, which shows the old lowering).**
  error.rs now runs the closures itself, `rt.xpcall(f, rt.handler, args…)`,
  with the status code and a fixed number of values in locals (no table;
  `rt.try` and its tables remain for a `try` whose closures end with a call
  whose values are all returned, such as a `try` that is a function's
  tail, and for a file that would otherwise exceed LuaJIT's limit on
  locals). crates/nl_ir/src/lower/lift.rs then makes each closure once: as
  an entry of a table of the main function when it keeps no variable of a
  narrower scope, otherwise right after the latest declaration of a
  variable it must keep (in a local of its own, or in a table of that
  function after the first four), within the limits on locals and upvalues
  that limits.rs enforces. Variables nothing else can change while the
  closure runs are passed as arguments; the others (assigned by the closure
  or by another function) stay upvalues. Locals assigned by the region keep
  today's semantics, so no values are threaded back. A visible difference:
  LuaJIT's error messages name a passed variable as a parameter of the
  lifted closure, so calling a nil `f` read in a `try` body now says
  "attempt to call local 'f'" where it said "upvalue 'f'". Measured with `nlc
  run --backend=bc` on stock LuaJIT (level 0, minimum of 3, a loaded
  machine), against the Lua twins on the fork: `try_ok` interpreter 2.78 s
  → 1.07 s (Lua 0.10 s; the same loop without the `try` takes 0.88 s
  there, the rest is level 0's comparison helpers), JIT at scale 4 6.58 s
  → 0.054 s (Lua 0.051 s, 1.04×); `try_raise` interpreter 2.92 → 2.78 s,
  JIT 2.37 → 2.22 s (unwinding and the trace still dominate). A `try`
  whose body assigns a local declared inside the same loop body still makes
  its body closure on every iteration (`let v = 0; try v = f(i) …`; the
  expression form `let v = try f(i) …` does not): JIT 1.54 → 0.62 s for
  that shape of `try_ok`, against 0.019 s at scale 1 for the expression form.

### Blocks in loops

- **Today (before this change).** A block `each xs do |v| … end` or an
  `fn` literal compiles to `FNEW` where it is written, plus `UCLO` at the
  end of the loop body when it captures a loop variable. The recorder does
  not compile either (`-jv`: `NYI: bytecode FNEW`), so a loop that holds a
  block literal runs in the interpreter, however hot: a 26× cliff for a
  loop whose work is one call of a block (`block_nocap` below: 0.739 s
  against 0.028 s with the closure made outside the loop, at 40M
  iterations). Lua has the same cliff; NL, a block-oriented language in
  Ruby's style, meets it everywhere.
- **Options.** (1) Inline the callee and the block at the call: no
  closure at all, but the frames of both leave `err.trace` and
  tracebacks, and every entry rule of the callee (defaults, keyed
  arguments, checks) has to be reproduced. (2) Make the closure once, as
  lift.rs does for `try`: exact except for function identity. (3) Teach
  the recorder `FNEW` (and `UCLO`): `lj_record.c` aborts on both; a trace
  would have to allocate a `GCfunc` and, for each captured local, a
  closed `GCupval` holding a copy (an open upvalue cannot alias a slot the
  trace keeps in a register), as `asm_cnew` allocates cdata, in both
  assembler backends, with the recorder specializing the call of the new
  closure by prototype (`PROTO_CLC_POLY`) since its identity changes each
  time; and the closure would still be allocated per iteration, so the
  hoisted form stays faster. Not attempted: weeks of VM work, x64 not
  testable here.
- **Applied: (2), restricted to where identity is unobservable.** The
  resolver records for every `fn` the parameters it only calls
  (`crates/nl_sema/src/callonly.rs`: calls, `p!` passed to another such
  parameter, uses inside blocks that are themselves passed that way,
  solved as a fixpoint across the file's functions; not for `async`
  functions; carried with exports). A block or `fn` literal written where
  a `fn` declaration or an imported `pub fn` receives it in such a
  position is made once by lift.rs: in the main function when it captures
  nothing of the loop, otherwise with the loop variables it captures
  *rebound*, a copy of each declared beside the closure and assigned right
  before the statement that holds the block (`k = i; s = s + apply(i,
  block)`). The copy is exact when nothing can assign the variable during
  the call (the variable's function is suspended in it, and no other
  function assigns it). A block inside a block is handled the same way,
  the copies living in the function around both. Methods (a descendant
  could override one with a parameter that is not call-only), Lua
  functions, variables holding functions, callees that keep or compare the
  argument, and blocks that assign a variable declared inside the loop are
  left as they are.
- **Measured** on this branch (arm64, JIT build, min of 5, checked NL
  against the base commit's compiler; 10M iterations unless noted):

  | program | Lua twin | before | after | note |
  |---|---|---|---|---|
  | `block_nocap` (`apply(i) do \|v\| v * 2 + 1 end`) | 0.182 | 0.225 | 0.010 | Lua with the closure made outside the loop: 0.009 |
  | `block_loopvar` (the block reads `i`) | 0.325 | 0.370 | 0.011 | `i` rebound |
  | `block_acc` (accumulator declared outside the loop, 3M) | 0.169 | 0.191 | 0.020 | |
  | `block_acc_in` (accumulator declared inside the loop, 3M) | 0.144 | 0.161 | 0.159 | stays: the block assigns a loop local |
  | `block_each` (`each xs do … end`, 10 items, 1M) | 0.044 | 0.084 | 0.045 | |
  | `block_nested` (a block inside a block, 300k) | 0.055 | 0.078 | 0.015 | both made once |
  | `block_two_levels` (`through(xs, f!)` and `wrapped`, 500k each) | 0.063 | 0.119 | 0.074 | `through` traces; `wrapped` makes its inner block per call |
  | `block_method` (`bag.each do … end` on a struct, 1M) | 0.050 | 0.165 | 0.163 | stays: a method callee |
  | `block_escape` (the callee stores `f!`) | 0.335 | 0.368 | 0.372 | refused, as it must be |
  | `closures` (this directory, scale 1) | 0.458 | 0.558 | 0.433 | the block part is made once; `make_adder` stays, in Lua too |

  `-jv` on `block_nocap` after: one root trace for the loop and one for
  `apply`; before: `NYI: bytecode FNEW` eleven times, then blacklisted.
  The `blocks` pair of this directory pins the three fast shapes for
  `run.sh`.
- **What stays slow.** A block on a method or a declared Lua function
  (`table.sort(t, fn …)`), a helper that wraps the block in a block of its
  own (`wrapped` above: the inner block captures the helper's parameter,
  which a copy in the main function could not stand for across
  recursion), and a block that assigns a variable declared inside the
  loop. The first could be opened to methods with a runtime guard on the
  method's identity, or to declared functions with an annotation in the
  declaration file; the second by cloning the helper with the captured
  variables as extra parameters; the third needs the closure, since the
  variable is new on every iteration.

### Auto-call on member reads (`ISMETH`, `MSELF`)

- **Today.** A member read on an untyped receiver is
  `MOV tmp, recv; TGETS v, recv, "x"; ISMETH v, tmp; JMP; CALL` — on the
  fast path four dispatches against Lua's one `TGETS`. A member call is
  `MSELF` + `CALLM` against Lua's `MOV` + `TGETS` + `CALL`. A receiver with a
  declared struct type gets a plain `TGETS` (static resolution).
- **Measured.** With the JIT, nothing: the traces of `fields` and of Lua's
  twin are instruction for instruction the same (`-jdump`), the `ISMETH`
  guard folds away for non-function values, and `methods` is at 0.89.
  In the interpreter: `fields` 1.17 untyped against
  1.20 for the same program typed and unchecked (so about 1% for
  the auto-call test; the rest of the 15% is the struct instances, below),
  `methods` unchecked 1.10 against `methods_typed` unchecked
  1.01 (about 7% on a loop of three member calls), and the
  `interp` pair, a dispatch loop whose work is mostly `ins.op`/`ins.arg`
  reads, 1.12 (JIT 1.02).
- **Recommendation: a codegen tweak now; the fused opcode only for
  interpreter-only targets.** `ISMETH` can test the receiver's own slot, so
  the `MOV` belongs in the call branch: one dispatch less per read, no VM
  change. A fused "read member and test" opcode (the fallback) would
  remove the other two dispatches, worth up to about 10% on member-heavy
  interpreted loops like `interp` and nothing with the JIT; it is one opcode
  in two interpreters plus a recorder case that already exists in pieces
  (`TGETS` + `ISMETH`). Worth it only if hosts that run without the JIT
  (consoles, iOS) matter. Note that declaring the receiver's type to get the
  plain `TGETS` is today much slower in a checked build (`fields_typed`
  4.02 / 5.81), because of the check, not the read: see
  below.

## Other costs found (not opcode questions)

These are larger than any of the five above, and none needs a VM change.

- **`rt.nokw` at every function entry (checked builds).** A function
  without `**opts` starts with `rt.nokw("name", p1, p2, …)` (func.rs), a
  vararg Lua function that loops over its arguments. `closures`:
  6.12 checked against 1.01 unchecked; `methods`:
  2.47 against 1.10. With the JIT, its loop makes the
  caller's trace abort ("inner loop in root trace", then blacklisting in
  `raytrace`). The keyed table can only arrive as the last argument passed,
  so the test could be inline: one `getmetatable(p) == KW` test per
  parameter (or only for the last parameters), with no loop and no vararg.
  (A first attempt in a scratch runtime, a loop-free `rt.nokw(fname, a, b, c,
  d, ...)`, did not help: the call itself is the cost; the test needs to be
  inline.)
- **Type checks.** `rt.check(v, T, "…")` per typed parameter, annotated
  `let` and assignment goes `rt.check` → `isa` → `I.info[T]` → the kind's
  test, several Lua calls; `rt.ret` returns through `...` (`VARG`, NYI in the
  recorder) and `rt.retn` recurses. `methods_typed`: 8.97 checked
  against 1.01 unchecked. `fields_typed`: 4.02 /
  5.81 against 1.20 / 1.54. Inline the common
  cases: `type(v) == "number"` for primitives, `getmetatable(v) == S` before
  falling back to `rt.check` for structs, and a non-vararg `rt.ret`.
- **Struct construction.** `S { … }` is `rt.new(S, {…})`: the caller's
  table, then `obj` and a `known` table, a loop over the fields with
  `rt.isa` per typed field, a `pairs` loop over the given keys. `struct_new`:
  19.6 / 5.86 checked, 6.64 / 5.57 unchecked; it is most
  of `raytrace` (16.2 / 32.3). The loops in `rt.new` break the
  caller's traces. And the instances it makes are slower to use afterwards
  with the JIT: `fields` with plain NL tables instead of structs runs at
  Lua's speed, the struct version at 1.58, typed or not; reproduced in
  pure Lua by building the tables the way `rt.new` does (copying fields in a
  loop from an init table that is not sunk), so it is where the instances
  land in memory, between the garbage tables. For a struct the compiler
  knows, construction can be a table constructor (`TDUP`) with defaults
  filled in and checks inline, plus `setmetatable` through `KINTR`; the
  unknown-field error is already static for a literal.

  Landed in two steps (after the run of RESULTS.md, which shows `rt.new`
  with its loops). A literal of a struct of the same file is that table
  constructor (crates/nl_ir/src/lower/structs.rs). Every other literal (a
  struct from another file, by an imported name or as `Lib.Point`, an
  alias, a struct whose parent the compiler does not see, such as an
  exception, a `spawn do` block) still calls `rt.new`, now with the names
  of the fields written, and from the literal's second run `rt.new` calls
  a constructor that the runtime generates for the struct and those fields
  (runtime/nl_20_struct.lua, CONSTRUCTORS): no loop, the inline tests of
  generated code, one table with room for the fields that get a value; in
  a trace the literal's own table is sunk. With the JIT, checked,
  `struct_import` went from 14.4 to 1.00 (5.82 s to 0.405 s at scale 4;
  Lua 0.405 s), and a loop that only builds an imported `Q { x: i, y: 1.5 }`
  and reads it, which Lua's JIT sinks, from 212 to 1.01, without an abort
  in `-jv`; `struct_new`, `raytrace`, `fields` and `methods_typed` did not
  move. (In `struct_import`, where `rt.new` runs twice per iteration, its
  own function trace usually forms before the loop's, and `-jv` shows a
  side trace of it given up four times, "loop unroll limit reached",
  before the loop is recorded whole.) (In the interpreter `struct_import` is
  at 3.23, from 23 with `-joff`: two tables and two calls per literal.)
  Two points the measurements decided.
  A constructor is made per set of fields given: one table sized for every
  field of the struct was 15% slower than Lua's when an optional field is
  left out. And a trace recorded through one of a literal's first two
  runs must not be kept: it ends in the interpreter and stays the trace of
  the loop, 2× slower for good (`-Ohotloop=1` shows it on every run). So
  that path creates a closure, which the recorder does not compile
  (`FNEW`), and the loop is recorded again later; `jit.off` on the
  function did the same but crashed stock LuaJIT about once in 12 000 runs
  of a fuzz program under `nlc run --jit=hot`. What stays as slow as it
  was: a field whose type has no inline test (a record, an interface, a
  negation, a host type), for any literal, of the same file or not: a
  record field checked per construction is 82× Lua with the JIT.
- **`case`.** `when 3..9` builds a Range per test (`rt.range` + `rt.when`)
  where two comparisons would do; each table arm calls `rt.istable`; each
  arm is a one-shot loop (`LOOP` … `JMP`), whose hot counter starts traces
  that abort with "leaving loop in root trace". `case_match`: 6.81 /
  1.27 (unchecked 4.82 / 1.34).
- **NL's string library** (`str_methods`, 4.56 / 3.21) is Ruby's
  semantics over UTF-8: every call checks the encoding, `reverse` builds a
  list of characters. That is the price of the library, not of the lowering;
  ASCII fast paths in `nl_80_string.lua` would close part of it.

## Trace cliffs in the runtime library (after the run)

None of the 20 pairs reaches these. They were found with micro-pairs, one
idiom in a loop of 5 to 20 million iterations against the Lua a programmer
would write, and with `-jv`: in each, the recorder gave up inside a runtime
helper, the loop was blacklisted and ran in the interpreter. Four causes:

- **A loop in a helper** ("inner loop in root trace"). A `while` aborts the
  trace that calls it at once; a counted `for` only when it comes round, so
  one that returns or ends in its first round is harmless. The walks up the
  struct chain (`find_def` for an operator, `x is I` for an interface, the
  `To`/`From` lookups, host types) and the loops over the parts of a union
  and the fields of a record type were of this kind.
- **A closure made per call.** `FNEW` is not compiled, and neither is the
  `UCLO` before every return of a function that contains one, even on a
  path that makes none. `rt.iter` made one for a table, a string and a
  range; the interface and conversion lookups passed one to a helper.
- **Tail calls** ("loop unroll limit reached"), the cause of the `bitops`
  figure above, at most 15 in a trace. An operator of a struct took three
  to reach its definition and one for every struct description read
  (`I.info`), so a loop body with five of them was over the limit.
- **A C function** the recorder does not know: the trace is stitched, or
  ends when the caller is a metamethod. `host.vm.ismethod` was asked about
  every value read through `rt.mget`, and `for ch in s` checks the encoding.

What changed: a struct's description now holds the nearest operator
definition by event and the set of interfaces with what it inherits, kept
up to date where definitions and implementations are recorded
(runtime/nl_42_opdef.lua, nl_22_iface.lua), so both are one lookup; the
generic metamethods call the left operand's definition themselves, and the
helpers that call nothing of the user's no longer end in tail calls; the
lookups take a function made once; unions and records of up to four parts
are tested without a loop; and `for v in x` over a value of unknown kind
keeps the iterator's control in a hidden variable, so that `rt.iter`
answers with a function made once for a table, a string and a range
(crates/nl_ir, lower/control.rs).

NL/Lua with the JIT, minimum of 5, before and after (scale 4; the machine
was loaded, so the last digit means little):

| idiom | before | after |
|---|---|---|
| `a + d * 0.5`, `fn Vec.+` | 190 | 0.6 |
| the same, operators inherited from a parent struct | 220 | 0.6 |
| six operators of a struct in the loop body | 520 | 0.7 |
| a parameter `s: HasArea` | 190 | 1.0 |
| the same, the interface implemented by a parent struct | 250 | 1.0 |
| a parameter `s: Named \| HasArea`, the value the second | 510 | 1.1 |
| a parameter `p: {x: Number, y: Number}` | 31 | 1.1 |
| `add(o.a, o.b)`, plain fields of an untyped `o` | 8.3 | 1.1 |
| `for v in xs`, four items, in a function called from the loop | 23 | 1.1 |
| `for j in r`, a range in a variable | 19 | 1.4 |
| `for ch in s`, eleven ASCII characters | 27 | 2.3 to 3.7 |
| `v as Number`, `v` a string half of the time | 29 | 3.0 |
| `m as Feet` through `To<Feet>` and `1.5 as Meters` through `From<Number>` | 510 | 28 |

What is left of the last four is compiled code: the range iterator is a
call per item where Lua has a numeric `for` (the Lua twin itself runs at
0.04 s or 0.11 s from one start to the next); a character is still a
string per item where Lua compares bytes; `as` reads two type descriptions
and two implementation records before it reaches `tonumber`; and the
`From` path builds its list of candidates on every conversion. Not fixed:

- `add(o.a, o.f)` where `f` holds a plain function: 7 before and after.
  The method mark of a function is the VM's, behind a C function; the
  compiler could emit `ISMETH` in a position that takes several values
  as it does elsewhere.
- `s[i]` on a string ends the trace: the runtime function is run by the
  VM as a metamethod and tail-calls a C primitive. A loop with `s[i]` in
  its body ran at 7 times Lua's `sub` after the changes above.
- A conversion found on an ancestor (`To`, `From`) still walks the chain
  in a loop, as does the test of a union or a record of more than four
  parts.
- `rt.new` for a struct the compiler does not see whole (one from an
  import): loops over the fields and the given keys, 170 times Lua's
  constructor.
- `rt.when` still ends in tail calls: a `case` with many `when` values
  that are not literals can reach the limit.

## Localized library paths

A Lua path such as `:math.floor(x)` used to compile to what Lua writes for
`math.floor(x)`: `GGET "math"` and `TGETS "floor"` at every use, two hash
lookups. Lua programmers avoid them with a local per library function, and
LuaJIT 3.0 proposes binding module functions when a chunk loads (issue
1092). The compiler now does it
(`crates/nl_ir/src/lower/localize.rs`): by default the fields of the standard
libraries that a file reads are read once, after its imports, into hidden
locals, and a use is a `UGET` in a function, the same bytecode as the Lua
idiom. `NL_OPT_LOCALIZE` 0 (`nlc --localize=none`) is the old code, and
`nlbench` takes it from `NLB_LOCALIZE`, so both can be timed in one build.

`bench/run.sh lua_path`, 3 runs (`NLB_RUNS=3 NLB_NOWAIT=1`; load average
about 3 on the 18 cores, other agents building), before with
`NLB_LOCALIZE=0` and after with the default, net ratios against the Lua twin:

| mode | before (`none`) | after (`std`) | unchecked, before / after |
|---|---|---|---|
| interp | 1.20 (0.266 s / 0.222 s) | **1.02** (0.226 s / 0.222 s) | 1.19 / 0.99 |
| JIT (scale 8) | 1.01 | **0.99** | 1.00 / 1.00 |

The interpreter paid 20% for two hash lookups per call and now runs at the
idiom's speed. Under the JIT the reads were already cheap (guarded loads
with constant keys, which the loop optimization can hoist), and the
immutable upvalue is now a constant of the trace; both are at parity. No existing pair writes a Lua
path (they use the prelude names, `Number.floor`, `OS.getenv`), so no other
row of RESULTS.md changes.

## Notes

- Load: other agents were building on this machine. The runner waited for
  `cargo`/`rustc`/`cc`/`clang`/`make` before timing (RESULTS.md says how
  long), but processes it does not look for kept running (one `nlc` process
  of another worktree used a core for the whole run). This is an 18-core
  machine with performance and efficiency cores; a process that lands on an
  efficiency core runs about 2.6× slower, which is what the 10% rule and the
  minima guard against.
- The checksums of every pair agreed in both builds and both NL modes.
- The JIT times of pairs where NL is far slower are short on the Lua side;
  the *net* column is the better ratio there.
