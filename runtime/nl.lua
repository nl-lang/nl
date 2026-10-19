-- The NL runtime library: what compiled NL code
-- relies on, and NL's standard library.
--
-- One chunk, opened once per state:
--
--   local rt = chunk(host)
--
-- The chunk is the concatenation of the files `runtime/*.lua` in the order
-- of their names. Both embedders build it that way, so the library can be
-- split into several files without either of them changing.
--
-- It installs the prelude globals and returns the runtime
-- table `rt`. It creates no other global. The embedder keeps `rt`: the VM
-- fork in the registry, `nlc run` on stock LuaJIT in Rust.
--
-- HOST TABLE. What the embedder provides. The runtime never reads a file or
-- compiles by itself.
--
--   host.compile(source, chunkname) -> chunk | nil, message
--       NL source to a loadable chunk (crates/nl_capi/src/driver.rs).
--       `message` is the first error as `file:line:column: text`, and
--       says how many more there are.
--   host.resolve(from, path) -> name | nil
--       The canonical name of the file an `import` path means, searched next
--       to the importing file, then in the working directory, then on
--       the search path. `from` is the canonical name of the
--       importing file, or nil.
--   host.read(name) -> source | nil
--   host.add_path(dir)
--       Appends to the search path that `host.resolve` uses.
--   host.canonical(path) -> name | nil          (optional)
--       The canonical name of the file at `path`, taken as given: with its
--       extension, relative to the working directory, no search. It is the
--       name `host.resolve` gives for the same file, and `host.read`
--       accepts it; nil when there is no such file. `include` uses it, so
--       that a file has one identity however its path is spelled. Without
--       it, `include` names the file by the path as written.
--   host.text                                   (optional; the fork)
--       The C glue over the text library of libnlc (LuaJIT/src/lib_nltext.c):
--       the primitives of NL's String library, listed in nl_80_string.lua.
--       Without it the String library uses its Lua stand-ins, which have no
--       Unicode case tables.
--   host.regex                                  (optional; the fork)
--       The C glue over the regex engine of libnlc (LuaJIT/src/lib_regex.c):
--       compile, test, find, captures, names, pattern, and `metatable`, the
--       metatable of compiled patterns (nl_82_regex.lua). Without it
--       `Regex.new` raises.
--   host.sandbox(f) -> f                        (optional; the fork)
--       While the sandbox option is on (NL_OPT_SANDBOX), gives
--       f the sandbox's global environment; otherwise does nothing.
--       `rt.load` passes every chunk from `host.compile` through it.
--       In the sandbox `host.compile` refuses a bytecode chunk.
--   host.loadfile(path) -> function | nil, message   (optional; the fork)
--       `loadfile`, except that in the sandbox the file must be Lua source,
--       is read through the host's loader when there is one, and gets the
--       sandbox's environment. `include` of a Lua file uses
--       it; without it, `loadfile`.
--   host.vm                                     (optional; the fork)
--       The VM's method mark: `markmethod(f)` sets it
--       on a function, `ismethod(f)` reads it. The runtime marks every
--       method both here and in its own weak set, so that the opcodes
--       ISMETH/MSELF and the Lua helpers agree.
--       The runtime table itself provides the VM with what its slow paths
--       need: rt.String, rt.strindex, rt.opgeneric and rt.arith
--       (LuaJIT/src/nlc.h, NLC_INTRINSICS).
--
-- The runtime takes what it uses of the Lua environment (library
-- functions, `loadstring`, `debug.getmetatable`, `io.stderr`, …) once,
-- when the chunk opens; a script that replaces a global or a library field
-- afterwards does not change what the runtime does. Where the `debug`
-- library offers `getmetatable` and `setmetatable` the runtime uses them,
-- so that a metatable locked with `__metatable` is still seen. With that
-- `loadstring` the runtime also loads the constructors it generates for
-- struct literals (nl_20_struct.lua), in a sandboxed state too (the
-- sandbox limits what scripts load); they read no global.
--
-- LOADED CHUNKS. A chunk from `host.compile` (a bytecode dump on the fork,
-- Lua source from the oracle backend) is loaded in two steps:
--
--   local main = assert(loadstring(chunk, chunkname))(rt)
--
-- `main()` runs the file and returns its export table. A file's chunk name
-- is "@" followed by its canonical name.
--
-- HOST INTERFACE. Fixed names that the C API (nl.h) calls.
-- Each has the meaning of the `nl_*` function named beside it.
--
--   rt.typeof(v) -> string                      nl_typeof
--   rt.isa(v, T) -> boolean                     nl_isa
--   rt.gettype(name) -> type | nil              nl_gettype
--   rt.settype(mt, name, parent_mt | nil)       nl_settype
--   rt.new(T, fields) -> instance               nl_new
--   rt.range(first, last, exclusive) -> range   nl_pushrange
--   rt.torange(v) -> first, last, exclusive     nl_torange; nothing when v is not a range
--   rt.typeerror(narg, expected, v)             nlL_typeerror, nlL_checkisa; raises TypeError.
--                                               `expected` is a type name or a type
--   rt.tostring(v) -> string                    nlL_tolstring
--   rt.handler(e) -> exception                  the message handler `try` and nl_pcall give to xpcall
--   rt.raise(v)                                 nl_raise
--   rt.exception(typename | nil, message) -> exception      nlL_raise, without raising
--   rt.settranslator(f)                         nl_settranslator
--   rt.newtask() -> task                        nl_newtask
--   rt.resolve(task, v)                         nl_resolve
--   rt.reject(task, e)                          nl_reject
--                                               Both run the task's waiters before the
--                                               outermost call returns: one made while
--                                               waiters run (by a resumed task, a callback)
--                                               leaves its waiters to that outer call
--   rt.taskstate(task) -> "pending" | "done", value | "failed", exception      nl_taskstate
--   rt.settimer(f)                              nl_settimer; f(seconds, wake), and wake() ends the sleep
--   rt.setunhandled(f)                          nl_setunhandled; nil restores the default hook
--   rt.unhandled(e)                             reports e through that hook
--   rt.pending() -> number                      tasks that have not finished (`nlc run` pumps until 0)
--   rt.import(path) -> exports                  nl_import
--   rt.load(source, chunkname) -> function | nil, message   load_nl, nl_loadbuffer
--   rt.addpath(dir)                             nl_addpath, add_path
--
-- EVENTS. The fork opens the C `event` library before this chunk runs
-- (lib_event.c). When the global `event` is absent (stock LuaJIT), this
-- chunk installs a Lua stand-in with the same three functions, which
-- follows the rules of the C library (nl_62_event.lua) and is also
-- `package.loaded.event`.

--
-- ADDITIONS. Beyond the fixed names above, `rt` holds the
-- intrinsics that generated code calls (`rt.mcall`, `rt.check`, …; listed
-- in each file's header) and these names for embedders:
--
--   rt.runmain(main, chunkname)                 runs a file's main function; an uncaught
--                                               error is re-raised as a message with its trace
--   rt.pump() -> boolean                        delivers what is queued (job results of the
--                                               `spawn` stand-in, posted callbacks); false when
--                                               there was nothing to do. Raises nothing: each
--                                               item runs protected, and an error goes to the
--                                               unhandled hook
--   rt.post(f) -> true                          queues f() for the next rt.pump (nl_post)
--   rt.searcher(modname) -> loader | message    the `.nl` searcher of `require`; the fork's
--                                               sandbox `require` calls it (lj_nlsandbox.c)
--
-- LAYOUT. This file holds the header and the core shared by the others:
-- the locals below are visible in every later file, since the files form
-- one chunk. Each later file is one `do ... end` block, so its own locals
-- end with it (LuaJIT allows 200 locals per function). Files share state
-- through `rt` (public) and `I` (internal, never returned).
--
--   nl.lua             core: shared locals, method marks
--   nl_10_types.lua    type descriptors, checks, `is`, `typeof`, host types
--   nl_20_struct.lua   structs, enums, modules
--   nl_22_iface.lua    interfaces, the `To` and `From` conversions
--   nl_30_member.lua   member access, auto-call, member calls, primitive methods
--   nl_40_ops.lua      operator dispatch, operators without stock events, ranges, iteration, `as`
--   nl_42_opdef.lua    operator definitions: metamethods, inheritance, `[]`
--   nl_45_pattern.lua  tests used by pattern matching
--   nl_50_error.lua    exceptions, `raise`, the message handler, `try`
--   nl_60_task.lua     tasks, the pump
--   nl_62_event.lua    the `event` stand-in
--   nl_64_worker.lua   workers and the `spawn` stand-in
--   nl_70_module.lua   imports, `load_nl`, `include`, `add_path`, the package searcher
--   nl_80_string.lua   NL's `String` library (host.text on the fork, a Lua stand-in otherwise)
--   nl_82_regex.lua    NL's `Regex` (host.regex on the fork; `Regex.new` raises otherwise)
--   nl_90_prelude.lua  prelude globals, file-handle methods, primitive metatables
--   nl_99_return.lua   returns `rt`

local host = ...

local rt = {}
local I = {}

-- Everything the runtime uses of the Lua environment, taken once, here
-- (see the header). Library tables are kept for the few that are shared
-- with scripts on purpose (`math` is NL's `Number`); functions are taken
-- one by one.
local type, pairs, ipairs, next, select = type, pairs, ipairs, next, select
local rawget, rawset, rawequal = rawget, rawset, rawequal
local tostring, tonumber, error, pcall, xpcall = tostring, tonumber, error, pcall, xpcall
local unpack = unpack or table.unpack
local loadstring, loadfile = loadstring or load, loadfile
local lstring, math, table, coroutine = string, math, table, coroutine
local bit = bit or require("bit")
local ldebug = type(debug) == "table" and debug or {}
-- `getmetatable` is the raw one where the debug library has it: a
-- metatable with a `__metatable` field is still the value's type.
local setmetatable = setmetatable
local getmetatable = ldebug.getmetatable or getmetatable
local debug_setmetatable = ldebug.setmetatable
local traceback = ldebug.traceback
local ssub, sfind, smatch, sgsub, sformat = lstring.sub, lstring.find, lstring.match, lstring.gsub, lstring.format
local tconcat, tsort, tremove = table.concat, table.sort, table.remove
local floor, huge = math.floor, math.huge
local cocreate, coresume, costatus, coyield, corunning =
  coroutine.create, coroutine.resume, coroutine.status, coroutine.yield, coroutine.running
local stderr = io and io.stderr
local stderr_write = stderr and stderr.write

-- Methods: on stock LuaJIT a function is marked as a method
-- by keeping it in this weak set; on the fork the
-- VM's bit is set as well (host.vm), so ISMETH and MSELF see it.
I.methods = setmetatable({}, { __mode = "k" })
local vm = type(host) == "table" and type(host.vm) == "table" and host.vm or nil
local vm_markmethod = vm and vm.markmethod
local vm_ismethod = vm and vm.ismethod

local function mark(f)
  I.methods[f] = true
  if vm_markmethod then vm_markmethod(f) end
  return f
end
I.mark = mark
rt.method = mark

-- Whether `f` is a method: either mark. False for anything that is not a
-- function.
function I.is_method(f)
  if I.methods[f] then return true end
  return vm_ismethod ~= nil and vm_ismethod(f) or false
end

-- A function that raises "not implemented" for a part of the runtime that
-- is not written yet.
function I.unimplemented(name)
  return function()
    error("NL runtime: " .. name .. " is not implemented yet", 2)
  end
end
