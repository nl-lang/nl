-- Tests of the NL opcodes of the fork (lj_bc.h BCDEF_NL, lj_bcdump.h BCDUMP_F_NL) on the interpreter. Each prototype is
-- assembled by hand (bcasm.lua) and loaded as a bytecode dump, so the tests
-- need no compiler: they state what the VM does for KINTR, ISMETH, MSELF
-- with CALLM/CALLMT, CMPGT/CMPNGT/CMPGE/CMPNGE, __len on tables, and the
-- dump loader's flag checks. Part 1 validates the assembler on stock
-- opcodes first.
--
-- tests/vm/nlops/run.sh runs it on the fork with the JIT on, off (-joff and
-- the LUAJIT_DISABLE_JIT build) and forced (-Ohotloop=1). The trace recorder
-- has no support for these opcodes yet: with the JIT on, traces touching
-- them abort and the interpreter must still give the right answers.

local dir = (arg and arg[0] or ""):match("^(.*)/") or "."
package.path = dir .. "/?.lua;" .. package.path
local asm = require("bcasm")

local passed, failed = 0, 0
local function test(name, f)
  local ok, err = pcall(f)
  if ok then passed = passed + 1; print("ok - " .. name)
  else failed = failed + 1; print("not ok - " .. name .. ": " .. tostring(err)) end
end
local function eq(a, b, what)
  if a ~= b then error((what or "value") .. ": got " .. tostring(a) .. ", expected " .. tostring(b), 2) end
end
local function raises(pattern, f, ...)
  local ok, err = pcall(f, ...)
  if ok then error("expected an error", 2) end
  if pattern and not tostring(err):find(pattern) then
    error("error " .. tostring(err) .. " does not mention " .. pattern, 2)
  end
  return err
end
local pack = function(...) return { n = select("#", ...), ... } end
local function same(got, ...)  -- got: a pack; ...: the expected values
  local want = pack(...)
  eq(got.n, want.n, "count")
  for i = 1, want.n do eq(got[i], want[i], "value " .. i) end
end

local P, UV = asm.PRI, asm.UV_LOCAL
assert(type(String) == "table", "no global String: not the real NL runtime")

-- Part 1: the assembler on stock opcodes ------------------------------------

test("assembler opcode numbers match jit/vmdef.lua", function()
  local ok, why = asm.check_vmdef()
  if ok == nil then error("cannot check: " .. why) end
end)

test("stock: KSHORT and RET1", function()
  eq(asm.fn{ framesize = 1, bc = { {"KSHORT", 0, 7}, {"RET1", 0, 2} } }(), 7)
  eq(asm.fn{ framesize = 1, bc = { {"KSHORT", 0, -3}, {"RET1", 0, 2} } }(), -3)
  same(pack(asm.fn{ framesize = 1, bc = { {"RET0", 0, 1} } }()))
end)

test("stock: string constants (KSTR, highest index written first)", function()
  local f = asm.fn{ framesize = 2, kgc = { "hello", "world" },
    bc = { {"KSTR", 0, 0}, {"KSTR", 1, 1}, {"RET", 0, 3} } }
  same(pack(f()), "hello", "world")
end)

test("stock: number constants (KNUM: integers and doubles)", function()
  local k = { 3.5, 1000000, -7, 2^53, -0.5, 1e300, 2^31 - 1, -2^31, 2^31, 0.1, 100 }
  local bc = {}
  for i = 1, #k do bc[#bc + 1] = {"KNUM", i - 1, i - 1} end
  bc[#bc + 1] = {"RET", 0, #k + 1}
  local got = pack(asm.fn{ framesize = #k, knum = k, bc = bc }())
  same(got, unpack(k))
  -- byte-exact with LuaJIT's writer for an integer and a double
  local real = string.dump(function() return 3.5, 1000000 end, true)
  local kb = asm.knumbytes(3.5) .. asm.knumbytes(1000000) .. "\0"  -- then EOF
  eq(real:sub(-#kb), kb, "knum bytes")
end)

test("stock: child prototype with an upvalue (FNEW, UGET, UCLO)", function()
  local child = asm.proto{ framesize = 1, uvdata = { UV + 0 },
    bc = { {"UGET", 0, 0}, {"RET1", 0, 2} } }
  local f = asm.fn{ framesize = 2, kgc = { child },
    bc = { {"KSHORT", 0, 5}, {"FNEW", 1, 0}, {"UCLO", 0, "out"}, "out:", {"RET1", 1, 2} } }
  local g = f()
  eq(type(g), "function")
  eq(g(), 5)
  -- two children: constant 1 is the second one written
  local c0 = asm.proto{ framesize = 1, bc = { {"KSHORT", 0, 10}, {"RET1", 0, 2} } }
  local c1 = asm.proto{ framesize = 1, bc = { {"KSHORT", 0, 11}, {"RET1", 0, 2} } }
  local h = asm.fn{ framesize = 2, kgc = { c0, c1 },
    bc = { {"FNEW", 0, 0}, {"FNEW", 1, 1}, {"RET", 0, 3} } }
  local f0, f1 = h()
  eq(f0(), 10); eq(f1(), 11)
end)

test("stock: conditional jump (IST, JMP)", function()
  local f = asm.fn{ numparams = 1, framesize = 2, bc = {
    {"IST", 0, 0}, {"JMP", 1, "yes"},
    {"KSHORT", 1, 1}, {"RET1", 1, 2},
    "yes:", {"KSHORT", 1, 2}, {"RET1", 1, 2} } }
  eq(f(true), 2); eq(f(0), 2); eq(f(false), 1); eq(f(nil), 1)
end)

test("stock: vararg function (VARG, RETM)", function()
  local f = asm.fn{ vararg = true, framesize = 2, bc = { {"VARG", 0, 0, 0}, {"RETM", 0, 0} } }
  same(pack(f()))
  same(pack(f(1, nil, 3)), 1, nil, 3)
end)

test("stock: calls (CALL, CALLM, CALLT)", function()
  -- f(g, a) = g(a, a) via CALL
  local f = asm.fn{ numparams = 2, framesize = 6, bc = {
    {"MOV", 2, 0}, {"MOV", 4, 1}, {"MOV", 5, 1}, {"CALL", 2, 0, 3}, {"RETM", 2, 0} } }
  local r = f(pack, 7)
  eq(r.n, 2); eq(r[1], 7); eq(r[2], 7)
  -- t(g, ...) = g(...) via CALLT
  local t = asm.fn{ numparams = 1, vararg = true, framesize = 4, bc = {
    {"MOV", 1, 0}, {"VARG", 3, 0, 1}, {"CALLMT", 1, 0} } }
  r = t(pack, 1, 2, 3)
  eq(r.n, 3); eq(r[3], 3)
end)

test("stripped dump has no debug info", function()
  local f = asm.fn{ framesize = 1, bc = { {"KSHORT", 0, 7}, {"RET1", 0, 2} } }
  local info = debug.getinfo(f, "S")
  eq(info.what, "Lua")
  eq(info.linedefined, 0)
end)

-- Part 2: the loader's flag checks ---------------------------------------------

local trivial = asm.proto{ framesize = 1, bc = { {"KSHORT", 0, 7}, {"RET1", 0, 2} } }

test("a dump with an unknown header flag (0x40) does not load", function()
  local dump = asm.dump(trivial, { flags = asm.F_STRIP + asm.F_FR2 + 0x40 })
  local f, err = loadstring(dump)
  eq(f, nil, "function")
  if not tostring(err):find("bytecode") then error("error does not mention bytecode: " .. tostring(err)) end
  raises("bytecode", asm.load, dump)
end)

test("a dump without BCDUMP_F_FR2 does not load", function()
  local dump = asm.dump(trivial, { flags = asm.F_STRIP })
  local f, err = loadstring(dump)
  eq(f, nil, "function")
  if not tostring(err):find("bytecode") then error("error does not mention bytecode: " .. tostring(err)) end
end)

test("a stock dump with BCDUMP_F_NL set loads on the fork", function()
  local f = asm.load(asm.dump(trivial, { nl = true }))
  eq(f(), 7)
end)

-- Part 3: KINTR -------------------------------------------------------------------

local function intrinsic(i)
  return asm.fn{ nl = true, framesize = 1, bc = { {"KINTR", 0, i}, {"RET1", 0, 2} } }
end

test("KINTR 1 is the NL String library", function()
  eq(intrinsic(1)(), String)
end)

test("KINTR 2 is rt.strindex (a function), KINTR 3 rt.opgeneric (a table)", function()
  eq(type(intrinsic(2)()), "function")
  eq(type(intrinsic(3)()), "table")
end)

test("KINTR of the same index gives the same object every time", function()
  local f = intrinsic(2)
  eq(f(), f())
  eq(intrinsic(3)(), intrinsic(3)())
end)

test("KINTR without PF_NL on the proto works too (only the header flag matters)", function()
  local f = asm.load(asm.dump(asm.proto{ framesize = 1,
    bc = { {"KINTR", 0, 1}, {"RET1", 0, 2} } }))
  eq(f(), String)
end)

-- Part 4: ISMETH ----------------------------------------------------------------

-- ismeth(member, recv): true when the member is called as a method of the
-- receiver. ISMETH falls through into the next instruction when it is, and
-- takes the JMP when it is not.
local ismeth = asm.fn{ numparams = 2, framesize = 3, bc = {
  {"ISMETH", 0, 1}, {"JMP", 2, "no"},
  {"KPRI", 2, P["true"]}, {"RET1", 2, 2},
  "no:", {"KPRI", 2, P["false"]}, {"RET1", 2, 2} } }

-- A Lua function marked as a method through BCDUMP_PF_METHOD: returns its
-- arguments.
local marked = asm.fn{ method = true, vararg = true, framesize = 2,
  bc = { {"VARG", 0, 0, 0}, {"RETM", 0, 0} } }

local function proxy(methods)
  local ud = newproxy(true)
  getmetatable(ud).__methods = methods
  return ud
end

test("ISMETH: a marked C function is a method of any receiver", function()
  eq(ismeth(io.stdout.write, io.stdout), true)
  eq(ismeth(io.stdout.write, {}), true)
  eq(ismeth(io.stdout.write, nil), true)
end)

test("ISMETH: an unmarked function with a userdata receiver without __methods", function()
  eq(ismeth(print, io.stdout), false)
  eq(ismeth(print, newproxy(false)), false)
  eq(ismeth(print, newproxy(true)), false)
end)

test("ISMETH: any function is a method of a string, number or boolean", function()
  eq(ismeth(print, "str"), true)
  eq(ismeth(print, 5), true)
  eq(ismeth(print, 2.5), true)
  eq(ismeth(print, true), true)
  eq(ismeth(print, false), true)
  eq(ismeth(function() end, ""), true)
end)

test("ISMETH: an unmarked function is not a method of a table, nil or function", function()
  eq(ismeth(print, {}), false)
  eq(ismeth(print, nil), false)
  eq(ismeth(print, print), false)
  eq(ismeth(print, setmetatable({}, { __methods = true })), false, "a table's __methods does not count")
end)

test("ISMETH: a non-function member is never a method", function()
  eq(ismeth(5, "str"), false)
  eq(ismeth("f", "str"), false)
  eq(ismeth({}, 5), false)
  eq(ismeth(nil, true), false)
  eq(ismeth(setmetatable({}, { __call = function() end }), "str"), false)
end)

test("ISMETH: a userdata receiver whose metatable has __methods", function()
  eq(ismeth(print, proxy(true)), true)
  eq(ismeth(print, proxy(1)), true, "any true value")
  eq(ismeth(print, proxy(false)), false)
  eq(ismeth(print, proxy(nil)), false)
  eq(ismeth(5, proxy(true)), false, "not a function")
end)

test("ISMETH: a Lua function from a PF_METHOD prototype is marked", function()
  eq(ismeth(marked, {}), true)
  eq(ismeth(marked, nil), true)
  eq(ismeth(marked, io.stdout), true)
  local unmarked = asm.fn{ vararg = true, framesize = 2, bc = { {"VARG", 0, 0, 0}, {"RETM", 0, 0} } }
  eq(ismeth(unmarked, {}), false)
end)

-- Part 5: MSELF with CALLM and CALLMT ------------------------------------------------

-- Frame layout for a call at base b: b = callee, b+1 = frame link, b+2 =
-- receiver, b+3.. = the other arguments (BCDUMP_F_FR2). MSELF decides how
-- many arguments CALLM passes (MULTRES): with the receiver when the callee
-- is a method of it, otherwise without it.

-- mcall(callee, recv, a, b): all results of the call.
local mcall = asm.fn{ numparams = 4, framesize = 12, bc = {
  {"MOV", 4, 0}, {"MOV", 6, 1}, {"MOV", 7, 2}, {"MOV", 8, 3},
  {"MSELF", 4, 0, 3}, {"CALLM", 4, 0, 0}, {"RETM", 4, 0} } }

-- mcall1(callee, recv, a, b): the first result only (CALLM B = 2).
local mcall1 = asm.fn{ numparams = 4, framesize = 12, bc = {
  {"MOV", 4, 0}, {"MOV", 6, 1}, {"MOV", 7, 2}, {"MOV", 8, 3},
  {"MSELF", 4, 0, 3}, {"CALLM", 4, 2, 0}, {"RET1", 4, 2} } }

-- mvar(callee, recv, ...): the receiver is the only fixed argument (C = 1),
-- the varargs follow (B = 1).
local mvar = asm.fn{ numparams = 2, vararg = true, framesize = 8, bc = {
  {"MOV", 2, 0}, {"MOV", 4, 1}, {"VARG", 5, 0, 2},
  {"MSELF", 2, 1, 1}, {"CALLM", 2, 0, 0}, {"RETM", 2, 0} } }

-- mvar1(callee, recv, a, ...): receiver and one fixed argument (C = 2), then
-- the varargs.
local mvar1 = asm.fn{ numparams = 3, vararg = true, framesize = 9, bc = {
  {"MOV", 3, 0}, {"MOV", 5, 1}, {"MOV", 6, 2}, {"VARG", 7, 0, 3},
  {"MSELF", 3, 1, 2}, {"CALLM", 3, 0, 0}, {"RETM", 3, 0} } }

-- mtail(callee, recv, a, b): MSELF then a tail call.
local mtail = asm.fn{ numparams = 4, framesize = 12, bc = {
  {"MOV", 4, 0}, {"MOV", 6, 1}, {"MOV", 7, 2}, {"MOV", 8, 3},
  {"MSELF", 4, 0, 3}, {"CALLMT", 4, 0} } }

-- mvartail(callee, recv, ...): varargs then a tail call.
local mvartail = asm.fn{ numparams = 2, vararg = true, framesize = 8, bc = {
  {"MOV", 2, 0}, {"MOV", 4, 1}, {"VARG", 5, 0, 2},
  {"MSELF", 2, 1, 1}, {"CALLMT", 2, 0} } }

local function capture(...) return select("#", ...), ... end

test("MSELF/CALLM: an unmarked callee with a table receiver gets the arguments only", function()
  local t = {}
  same(pack(mcall(capture, t, 1, 2)), 2, 1, 2)
  same(pack(mcall(capture, t, nil, nil)), 2, nil, nil)
  same(pack(mcall(capture, nil, "a", "b")), 2, "a", "b")
  same(pack(mcall(capture, print, "a", "b")), 2, "a", "b")
  eq(mcall1(capture, t, 1, 2), 2)
end)

test("MSELF/CALLM: a string receiver makes an unmarked callee a method", function()
  same(pack(mcall(capture, "recv", 1, 2)), 3, "recv", 1, 2)
  same(pack(mcall(capture, 5, 1, 2)), 3, 5, 1, 2)
  same(pack(mcall(capture, 2.5, 1, 2)), 3, 2.5, 1, 2)
  same(pack(mcall(capture, false, 1, 2)), 3, false, 1, 2)
  eq(mcall1(capture, "recv", 1, 2), 3)
end)

test("MSELF/CALLM: a userdata receiver with __methods", function()
  local yes, no = proxy(true), proxy(false)
  same(pack(mcall(capture, yes, 1, 2)), 3, yes, 1, 2)
  same(pack(mcall(capture, no, 1, 2)), 2, 1, 2)
  same(pack(mcall(capture, newproxy(false), 1, 2)), 2, 1, 2)
  same(pack(mcall(capture, io.stdout, 1, 2)), 2, 1, 2)
end)

test("MSELF/CALLM: a marked callee gets the receiver whatever it is", function()
  local t = {}
  same(pack(mcall(marked, t, 1, 2)), t, 1, 2)
  same(pack(mcall(marked, nil, 1, 2)), nil, 1, 2)
  same(pack(mcall(marked, "s", 1, 2)), "s", 1, 2)
  -- a marked C function: the same as io.stdout:write("", "")
  same(pack(mcall(io.stdout.write, io.stdout, "", "")), io.stdout:write("", ""))
  -- and not a method call: write("", "") on no file is an error
  raises("", mcall, io.stdout.write, {}, "", "")
end)

test("MSELF/CALLM: a callable table is not a function: no receiver, __call runs", function()
  local callable = setmetatable({}, { __call = function(self, ...) return self, select("#", ...), ... end })
  same(pack(mcall(callable, "recv", 1, 2)), callable, 2, 1, 2)
  same(pack(mcall(callable, {}, 1, 2)), callable, 2, 1, 2)
end)

test("MSELF/CALLM: calling a non-callable value raises", function()
  raises("call", mcall, 5, "recv", 1, 2)
  raises("call", mcall, nil, {}, 1, 2)
end)

test("MSELF/CALLM with B = 1: vararg tail, method case keeps the receiver", function()
  same(pack(mvar(capture, "r")), 1, "r")
  same(pack(mvar(capture, "r", 1)), 2, "r", 1)
  same(pack(mvar(capture, "r", 1, 2, 3)), 4, "r", 1, 2, 3)
  same(pack(mvar(capture, "r", nil, nil)), 3, "r", nil, nil)
  local t = {}
  same(pack(mvar(marked, t, 1, 2, 3)), t, 1, 2, 3)
  same(pack(mvar(marked, t)), t)
end)

test("MSELF/CALLM with B = 1: vararg tail, non-method case drops the receiver", function()
  local t = {}
  same(pack(mvar(capture, t)), 0)
  same(pack(mvar(capture, t, 1)), 1, 1)
  same(pack(mvar(capture, t, 1, 2, 3)), 3, 1, 2, 3)
  same(pack(mvar(capture, t, nil, 2, nil)), 3, nil, 2, nil)
  same(pack(mvar(capture, nil, 1, 2)), 2, 1, 2)
end)

test("MSELF/CALLM with B = 1 and C = 2: a fixed argument before the varargs", function()
  local t = {}
  same(pack(mvar1(capture, t, "a")), 1, "a")
  same(pack(mvar1(capture, t, "a", "b", "c")), 3, "a", "b", "c")
  same(pack(mvar1(capture, "r", "a")), 2, "r", "a")
  same(pack(mvar1(capture, "r", "a", "b", "c")), 4, "r", "a", "b", "c")
  same(pack(mvar1(marked, t, "a", "b")), t, "a", "b")
end)

test("MSELF/CALLM: many varargs (past the fixed frame)", function()
  local args = {}
  for i = 1, 60 do args[i] = i end
  local r = pack(mvar(capture, {}, unpack(args)))
  eq(r.n, 61); eq(r[1], 60); eq(r[2], 1); eq(r[61], 60)
  r = pack(mvar(capture, "s", unpack(args)))
  eq(r.n, 62); eq(r[1], 61); eq(r[2], "s"); eq(r[62], 60)
end)

test("MSELF/CALLMT: a tail call after MSELF", function()
  local t = {}
  same(pack(mtail(capture, t, 1, 2)), 2, 1, 2)
  same(pack(mtail(capture, "r", 1, 2)), 3, "r", 1, 2)
  same(pack(mtail(marked, t, 1, 2)), t, 1, 2)
  local ud = proxy(true)
  same(pack(mtail(capture, ud, 1, 2)), 3, ud, 1, 2)
  same(pack(mtail(io.stdout.write, io.stdout, "", "")), io.stdout:write("", ""))
end)

test("MSELF/CALLMT with B = 1: vararg tail call", function()
  local t = {}
  same(pack(mvartail(capture, t)), 0)
  same(pack(mvartail(capture, t, 1, 2, 3)), 3, 1, 2, 3)
  same(pack(mvartail(capture, "r")), 1, "r")
  same(pack(mvartail(capture, "r", 1, 2, 3)), 4, "r", 1, 2, 3)
  same(pack(mvartail(marked, t, 1, 2)), t, 1, 2)
  same(pack(mvartail(marked, t)), t)
end)

test("MSELF/CALLMT: the tail call is a real tail call", function()
  local function count()
    local n = 0
    while debug.getinfo(n + 1, "l") do n = n + 1 end
    return n
  end
  local base = count()
  local function probe() return count() - base end
  -- through mtail the probe runs in mtail's frame: one level, not two
  eq(mtail(probe, {}, 1, 2), 1)
  eq(mtail(probe, "r", 1, 2), 1)
  eq(mcall(probe, {}, 1, 2), 2)
end)

-- Part 6: CMPGT, CMPNGT, CMPGE, CMPNGE -------------------------------------------

-- cmp(op)(a, b): true when the comparison takes its jump.
local function cmp(op)
  return asm.fn{ numparams = 2, framesize = 3, bc = {
    {op, 0, 1}, {"JMP", 2, "yes"},
    {"KPRI", 2, P["false"]}, {"RET1", 2, 2},
    "yes:", {"KPRI", 2, P["true"]}, {"RET1", 2, 2} } }
end
local gt, ngt, ge, nge = cmp("CMPGT"), cmp("CMPNGT"), cmp("CMPGE"), cmp("CMPNGE")
local nan = 0 / 0

test("CMPGT/CMPGE on numbers", function()
  eq(gt(2, 1), true); eq(gt(1, 2), false); eq(gt(1, 1), false)
  eq(ge(2, 1), true); eq(ge(1, 2), false); eq(ge(1, 1), true)
  eq(gt(-1, -2), true); eq(gt(-2, -1), false)
  eq(gt(2.5, 2), true); eq(gt(2, 2.5), false); eq(ge(2.5, 2.5), true)
  eq(gt(2^40, 2^31), true); eq(ge(1e308, -1e308), true)
  eq(gt(0, -0), false); eq(ge(0, -0), true)
  eq(gt(math.huge, 1e308), true); eq(gt(-math.huge, -1e308), false)
end)

test("CMPNGT/CMPNGE on numbers are the negations", function()
  eq(ngt(2, 1), false); eq(ngt(1, 2), true); eq(ngt(1, 1), true)
  eq(nge(2, 1), false); eq(nge(1, 2), true); eq(nge(1, 1), false)
  eq(ngt(2.5, 2), false); eq(nge(2.5, 2.5), false); eq(ngt(2, 2.5), true)
end)

test("CMPGT/CMPGE with NaN are false; CMPNGT/CMPNGE with NaN are true", function()
  eq(gt(nan, 1), false); eq(gt(1, nan), false); eq(gt(nan, nan), false)
  eq(ge(nan, 1), false); eq(ge(1, nan), false); eq(ge(nan, nan), false)
  eq(ngt(nan, 1), true); eq(ngt(1, nan), true); eq(ngt(nan, nan), true)
  eq(nge(nan, 1), true); eq(nge(1, nan), true); eq(nge(nan, nan), true)
end)

test("CMPGT/CMPGE on strings", function()
  eq(gt("b", "a"), true); eq(gt("a", "b"), false); eq(gt("a", "a"), false)
  eq(ge("b", "a"), true); eq(ge("a", "b"), false); eq(ge("a", "a"), true)
  eq(gt("ab", "a"), true); eq(gt("a", "ab"), false); eq(gt("a\0b", "a"), true)
  eq(gt("", ""), false); eq(ge("", ""), true); eq(gt("a", ""), true)
  eq(ngt("b", "a"), false); eq(ngt("a", "b"), true); eq(ngt("a", "a"), true)
  eq(nge("a", "b"), true); eq(nge("a", "a"), false)
  eq(gt("10", "9"), false, "strings compare as strings")
end)

test("CMPGT/CMPGE: a table with __gt/__ge gets (a, b) in that order", function()
  local log
  local mt = {
    __gt = function(a, b) log[#log + 1] = { "gt", a, b }; return a.v > b.v end,
    __ge = function(a, b) log[#log + 1] = { "ge", a, b }; return a.v >= b.v end,
  }
  local a, b = setmetatable({ v = 2 }, mt), setmetatable({ v = 1 }, mt)
  log = {}
  eq(gt(a, b), true); eq(gt(b, a), false); eq(gt(a, a), false)
  eq(#log, 3); eq(log[1][1], "gt"); eq(log[1][2], a); eq(log[1][3], b)
  eq(log[2][2], b); eq(log[2][3], a)
  log = {}
  eq(ge(a, b), true); eq(ge(b, a), false); eq(ge(a, a), true)
  eq(#log, 3); eq(log[1][1], "ge"); eq(log[1][2], a); eq(log[1][3], b)
  log = {}
  eq(ngt(a, b), false); eq(ngt(b, a), true); eq(nge(b, a), true); eq(nge(a, a), false)
  eq(#log, 4); eq(log[1][1], "gt"); eq(log[3][1], "ge")
end)

test("CMPGT/CMPGE: the metamethod's result is taken as a condition", function()
  local r
  local mt = { __gt = function() return r end, __ge = function() return r end }
  local a, b = setmetatable({}, mt), setmetatable({}, mt)
  r = 1; eq(gt(a, b), true); eq(ngt(a, b), false); eq(ge(a, b), true); eq(nge(a, b), false)
  r = "x"; eq(gt(a, b), true)
  r = nil; eq(gt(a, b), false); eq(ngt(a, b), true); eq(ge(a, b), false); eq(nge(a, b), true)
  r = false; eq(gt(a, b), false); eq(ngt(a, b), true)
end)

test("CMPGT with only __lt calls __lt(b, a)", function()
  local log = {}
  local mt = { __lt = function(x, y) log[#log + 1] = { x, y }; return x.v < y.v end }
  local a, b = setmetatable({ v = 2 }, mt), setmetatable({ v = 1 }, mt)
  eq(gt(a, b), true)
  eq(#log, 1); eq(log[1][1], b); eq(log[1][2], a)
  eq(gt(b, a), false)
  eq(log[2][1], a); eq(log[2][2], b)
  eq(ngt(a, b), false); eq(ngt(b, a), true)
  eq(#log, 4)
end)

test("CMPGE with only __le calls __le(b, a)", function()
  local log = {}
  local mt = { __le = function(x, y) log[#log + 1] = { x, y }; return x.v <= y.v end }
  local a, b = setmetatable({ v = 2 }, mt), setmetatable({ v = 1 }, mt)
  eq(ge(a, b), true)
  eq(#log, 1); eq(log[1][1], b); eq(log[1][2], a)
  eq(ge(b, a), false); eq(ge(a, a), true)
  eq(nge(a, b), false); eq(nge(b, a), true)
  eq(#log, 5)
end)

test("CMPGE with only __lt falls back to not __lt(a, b)", function()
  local log = {}
  local mt = { __lt = function(x, y) log[#log + 1] = { x, y }; return x.v < y.v end }
  local a, b = setmetatable({ v = 2 }, mt), setmetatable({ v = 1 }, mt)
  eq(ge(a, b), true)
  eq(#log, 1); eq(log[1][1], a); eq(log[1][2], b)
  eq(ge(b, a), false); eq(ge(a, a), true)
  eq(nge(a, b), false); eq(nge(b, a), true)
end)

test("CMPGT/CMPGE: __gt/__ge win over __lt/__le", function()
  local used
  local mt = {
    __gt = function() used = "gt"; return true end, __lt = function() used = "lt"; return true end,
    __ge = function() used = "ge"; return true end, __le = function() used = "le"; return true end,
  }
  local a, b = setmetatable({}, mt), setmetatable({}, mt)
  used = nil; gt(a, b); eq(used, "gt")
  used = nil; ge(a, b); eq(used, "ge")
  used = nil; ngt(a, b); eq(used, "gt")
  used = nil; nge(a, b); eq(used, "ge")
end)

test("CMPGT/CMPGE: a table and a number reach the table's metamethod on either side", function()
  local log = {}
  local mt = {
    __gt = function(x, y) log[#log + 1] = { "gt", x, y }; return true end,
    __ge = function(x, y) log[#log + 1] = { "ge", x, y }; return true end,
    __lt = function(x, y) log[#log + 1] = { "lt", x, y }; return true end,
    __le = function(x, y) log[#log + 1] = { "le", x, y }; return true end,
  }
  local t = setmetatable({}, mt)
  -- table on the left: its __gt/__ge, with (t, 5)
  eq(gt(t, 5), true); eq(ge(t, 5), true)
  eq(log[1][1], "gt"); eq(log[1][2], t); eq(log[1][3], 5)
  eq(log[2][1], "ge"); eq(log[2][2], t); eq(log[2][3], 5)
  -- number on the left: 5 has no __gt, so the swapped __lt/__le of t, (t, 5)
  log = {}
  eq(gt(5, t), true); eq(ge(5, t), true)
  eq(log[1][1], "lt"); eq(log[1][2], t); eq(log[1][3], 5)
  eq(log[2][1], "le"); eq(log[2][2], t); eq(log[2][3], 5)
  -- a string on the left behaves the same
  log = {}
  eq(gt("s", t), true)
  eq(log[1][1], "lt"); eq(log[1][2], t); eq(log[1][3], "s")
end)

test("CMPGT/CMPGE: a table with only __lt/__le against a number", function()
  local log = {}
  local mt = {
    __lt = function(x, y) log[#log + 1] = { "lt", x, y }; return true end,
    __le = function(x, y) log[#log + 1] = { "le", x, y }; return true end,
  }
  local t = setmetatable({}, mt)
  eq(gt(t, 5), true)   -- __lt(5, t)
  eq(log[1][1], "lt"); eq(log[1][2], 5); eq(log[1][3], t)
  eq(ge(t, 5), true)   -- __le(5, t)
  eq(log[2][1], "le"); eq(log[2][2], 5); eq(log[2][3], t)
  eq(gt(5, t), true)   -- __lt(t, 5)
  eq(log[3][1], "lt"); eq(log[3][2], t); eq(log[3][3], 5)
  eq(ngt(t, 5), false); eq(nge(5, t), false)
  eq(#log, 5)
end)

test("CMPGT/CMPGE: two tables with different metatables, left asked first", function()
  local log = {}
  local a = setmetatable({}, { __gt = function(x, y) log[#log + 1] = "a.gt"; return true end })
  local b = setmetatable({}, { __gt = function(x, y) log[#log + 1] = "b.gt"; return false end,
                               __lt = function(x, y) log[#log + 1] = "b.lt"; return true end })
  eq(gt(a, b), true); eq(log[1], "a.gt")
  eq(gt(b, a), false); eq(log[2], "b.gt")
  local c = setmetatable({}, {})
  eq(gt(c, b), true); eq(log[3], "b.lt", "the right operand's __lt when the left has none")
end)

test("CMPGT/CMPGE without a metamethod raise a comparison error", function()
  raises("compare", gt, {}, {})
  raises("compare", ge, {}, {})
  raises("compare", ngt, {}, 1)
  raises("compare", nge, "s", 1)
  raises("compare", gt, 1, "1")
  raises("compare", gt, nil, nil)
  raises("compare", gt, true, false)
end)

test("CMPGT/CMPGE: a metamethod that raises propagates its error", function()
  local t = setmetatable({}, { __gt = function() error("from __gt", 0) end })
  eq(raises("from __gt", gt, t, 1), "from __gt")
end)

-- Part 7: __len on tables ----------------------------------------------------------

local len = asm.fn{ numparams = 1, framesize = 2, bc = { {"LEN", 1, 0}, {"RET1", 1, 2} } }

test("__len on a table is honored by LEN in a loaded dump", function()
  local seen
  local t = setmetatable({ 1, 2, 3 }, { __len = function(x) seen = x; return 42 end })
  eq(len(t), 42)
  eq(seen, t)
  eq(len({ 1, 2, 3 }), 3)
  eq(len(setmetatable({ 1, 2 }, {})), 2)
  eq(len("four"), 4)
end)

test("__len on a table is honored by # in Lua source", function()
  local t = setmetatable({ 1, 2, 3 }, { __len = function() return 42 end })
  eq(#t, 42)
  eq(#setmetatable({}, { __len = function() return "x" end }), "x")
  local u = setmetatable({ 1, 2 }, {})
  eq(#u, 2)
  getmetatable(u).__len = function() return 7 end
  eq(#u, 7, "a __len added later")
end)

-- Part 8: repetition with the JIT on ---------------------------------------------------

-- With -Ohotloop=1 these loops become hot and the recorder meets the NL
-- opcodes; the traces must abort and the results must stay right.

test("hot loop: CMPGT/CMPGE over numbers, strings and metamethods", function()
  local function v(x) return type(x) == "table" and x.v or x end
  local mt = { __gt = function(a, b) return v(a) > v(b) end, __le = function(a, b) return v(a) <= v(b) end }
  local a, b = setmetatable({ v = 2 }, mt), setmetatable({ v = 1 }, mt)
  for i = 1, 200 do
    eq(gt(i, i - 1), true); eq(gt(i - 1, i), false); eq(ge(i, i), true)
    eq(ngt(nan, i), true); eq(nge(i, nan), true)
    eq(gt("b" .. i, "a" .. i), true)
    eq(gt(a, b), true); eq(ge(b, a), false); eq(gt(a, i), 2 > i, "__gt(a, i) compares a.v with i")
  end
end)

test("hot loop: ISMETH and MSELF", function()
  local t, ud = {}, proxy(true)
  for i = 1, 200 do
    eq(ismeth(print, t), false); eq(ismeth(print, "s"), true); eq(ismeth(marked, t), true)
    eq(ismeth(print, ud), true)
    same(pack(mcall(capture, t, i, 2)), 2, i, 2)
    same(pack(mcall(capture, "r", i, 2)), 3, "r", i, 2)
    same(pack(mvar(capture, t, i)), 1, i)
    same(pack(mvar(capture, ud, i)), 2, ud, i)
    same(pack(mtail(marked, t, i, 2)), t, i, 2)
  end
end)

test("hot loop: KINTR", function()
  local f, g = intrinsic(1), intrinsic(3)
  for i = 1, 200 do
    eq(f(), String)
    eq(type(g()), "table")
  end
end)

-- The recorder's LEN on a table (lj_record.c, BC_LEN: ALEN when !LJ_52)
-- does not know that this fork's interpreter honors __len on tables. The
-- tests below make `len` hot through its own call counter: `warm` calls it
-- from a function the JIT never compiles (jit.off), so with the JIT on the
-- calls after the warm-up run `len`'s compiled trace (warm returns once the
-- function header reads JFUNCF). They pass with -joff and on the
-- LUAJIT_DISABLE_JIT build.
local haveutil, jutil = pcall(require, "jit.util")
local function compiled(f)  -- a root trace patches the header to JFUNCF
  if not haveutil then return false end
  local ins = jutil.funcbc(f, 0)
  return ins ~= nil and ins % 256 == asm.ops.JFUNCF
end
local function warm(f, x, want)
  for i = 1, 5000 do
    eq(f(x), want, "during warm-up")
    if compiled(f) then return true end
  end
  if jit.status() then print("# note: the JIT did not compile the warmed-up function") end
  return false
end
pcall(jit.off, warm, true)  -- raises on a LUAJIT_DISABLE_JIT build
local function plen(x) return #x end  -- the same shape in plain Lua

test("JIT: a LEN trace recorded on plain tables must honor __len (dump)", function()
  local f = asm.fn{ numparams = 1, framesize = 2, bc = { {"LEN", 1, 0}, {"RET1", 1, 2} } }
  warm(f, { 1, 2, 3 }, 3)
  local t = setmetatable({}, { __len = function() return 42 end })
  eq(f(t), 42)
end)

test("JIT: a LEN trace recorded on plain tables must honor __len (Lua source)", function()
  warm(plen, { 1, 2 }, 2)
  local t = setmetatable({ 1 }, { __len = function() return 42 end })
  eq(plen(t), 42)
end)

test("JIT: a LEN trace recorded under __len must not fold its result (dump)", function()
  local f = asm.fn{ numparams = 1, framesize = 2, bc = { {"LEN", 1, 0}, {"RET1", 1, 2} } }
  local t = setmetatable({}, { __len = function() return 42 end })
  warm(f, t, 42)
  local u = setmetatable({}, { __len = function() return 7 end })
  eq(f(u), 7)
  eq(f({ 1, 2, 3 }), 3)
  eq(f("four"), 4)
end)

test("JIT: a LEN trace recorded under __len must not fold its result (Lua source)", function()
  local function plen2(x) return #x end
  local t = setmetatable({}, { __len = function() return 42 end })
  warm(plen2, t, 42)
  local u = setmetatable({}, { __len = function() return 7 end })
  eq(plen2(u), 7)
  eq(plen2({ 1, 2, 3 }), 3)
end)

print(("# %d passed, %d failed (%s, JIT %s)"):format(passed, failed, jit.version,
  jit.status() and "on" or "off"))
if failed > 0 then os.exit(1) end
