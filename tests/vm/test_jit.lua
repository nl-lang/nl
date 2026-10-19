-- Regression tests for faults of the VM in plain Lua: the trace compiler
-- must compute what the interpreter computes, and bytecode dumps must carry
-- the flags stock LuaJIT writes. Plain Lua only (it runs on stock LuaJIT,
-- too); traces are forced with hotloop=1 where the JIT is on.
--
-- tests/vm/run.sh runs it on the fork with the JIT on and off, and with
-- LuaJIT's assertions (which check the recorder's slots against the stack).

local passed, failed, skipped = 0, 0, 0
local function test(name, f)
  local ok, err = pcall(f)
  if ok and err == "skip" then skipped = skipped + 1; print("ok - " .. name .. " # skip")
  elseif ok then passed = passed + 1; print("ok - " .. name)
  else failed = failed + 1; print("not ok - " .. name .. ": " .. tostring(err)) end
end
local function eq(a, b, what)
  if a ~= b then error((what or "value") .. ": got " .. tostring(a) .. ", expected " .. tostring(b), 2) end
end

local jit_on = jit and jit.status and jit.status()
if jit_on then jit.opt.start("hotloop=1") end

-- math.floor and math.ceil -----------------------------------------------

-- On dual-number builds (arm64 among them) the recorder narrows the result
-- of math.floor and math.ceil to an integer, which cannot hold -0.
test("math.floor of a constant -0 in a trace is -0", function()
  local floor = math.floor
  for i = 1, 40 do eq(tostring(floor(-0.0)), "-0", "iteration " .. i) end
end)

test("math.floor and math.ceil of -0 in a trace are -0", function()
  local floor, ceil = math.floor, math.ceil
  local xs = { -0.0, -0.0, 2.5, -0.0, 1.5, -0.0 }
  local fl = { "-0", "-0", "2", "-0", "1", "-0" }
  local cs = { 1.5, -0.5, -0.0, 2.5, -0.25, -3.5 }
  local ce = { "2", "-0", "-0", "3", "-0", "-3" }
  for i = 1, 60 do
    local k = i % 6 + 1
    eq(tostring(floor(xs[k])), fl[k], "floor(" .. xs[k] .. ")")
    eq(tostring(ceil(cs[k])), ce[k], "ceil(" .. cs[k] .. ")")
    eq(1 / floor(xs[k]) < 0, fl[k]:sub(1, 1) == "-", "sign of floor(" .. xs[k] .. ")")
  end
end)

test("math.floor in a function called from a trace keeps -0", function()
  local function f(x) return tostring(math.floor(x)) end
  for i = 1, 40 do eq(f(-0.0), "-0", "iteration " .. i) end
end)

-- Unchecked integer arguments ----------------------------------------------

-- string.char, string.byte and the like convert their arguments to integers
-- without a check. Narrowing that conversion across + and - must not
-- convert an operand that is not an int32 integer on its own. The traces
-- are recorded while big + v exceeds int32; (big + v) % 26 is recorded as
-- x - floor(x / 26) * 26, and floor(x / 26) * 26 exceeds it as well.
test("an argument computed past int32 is exact", function()
  local big = 2147483647
  local vs = { 9, 9, 9, 0, 9, 1, 9, 9, 9, 9 }
  local want = { [0] = "x", [1] = "y", [9] = "g" }  -- (big + v) % 26 + 97
  for i = 1, #vs do
    local v = vs[i]
    eq(string.char((big + v) % 26 + 97), want[v], "v = " .. v)
    eq(string.byte("abcdefghijklmnopqrstuvwxyz", (big + v) % 26 + 1), want[v]:byte(), "byte, v = " .. v)
  end
end)

test("an argument with a non-integral operand is truncated as a whole", function()
  local xs = { 1.5, 2.5, -0.5, 3.25, -0.75 }
  local want = { "C", "D", "A", "E", "A" }  -- trunc(x + 66)
  for i = 1, 50 do
    local k = i % 5 + 1
    eq(string.char(xs[k] + 66), want[k], "x = " .. xs[k])
  end
end)

test("the sum of a table field past int32, as a member call would read it", function()
  local function chr(n) return string.char(n) end
  local big = 2147483647
  local out = {}
  for i, v in ipairs({ 409, 409, 409, 32, 409, 0, 409, 409 }) do
    out[i] = chr((big + ({ f = v % 100 }).f) % 26 + 97)
  end
  eq(table.concat(out), "gggdgxgg")
end)

-- Bit operators -------------------------------------------------------------

-- The flags of a dump: BCDUMP_F_BITOP (0x10) in the header when the main
-- function or a child uses a bit operator, 0x80 in such a prototype's own
-- flags; never for a function without them, however many closures of it
-- were made.
local function dumpflags(f)
  local d = string.dump(f, true)
  local pos, flags = 6, {}
  while true do
    local n, sh = 0, 0
    repeat
      local b = d:byte(pos); pos = pos + 1
      n = n + (b % 128) * 2 ^ sh; sh = sh + 7
    until b < 128
    if n == 0 then break end
    flags[#flags + 1] = d:byte(pos)
    pos = pos + n
  end
  return d:byte(5), flags[#flags], flags[1]  -- header, main, first child
end
local function hasbit(a, b) return a % (2 * b) >= b end

test("a function without bit operators dumps without their flags", function()
  local fs = {}
  for i = 1, 8 do fs[i] = function() return i end end
  local hdr, main = dumpflags(fs[1])
  eq(hasbit(hdr, 0x10), false, "header BCDUMP_F_BITOP")
  eq(hasbit(main, 0x80), false, "prototype flag")
end)

if loadstring("return 1 & 1") then
  test("a function with bit operators dumps with their flags", function()
    local g = loadstring("return function(x) return x & 1 end")()
    local hdr, main = dumpflags(g)
    eq(hasbit(hdr, 0x10), true, "header BCDUMP_F_BITOP")
    eq(hasbit(main, 0x80), true, "prototype flag")
    local h = loadstring("return function() return function(x) return ~x end end")()
    local hhdr, hmain, hchild = dumpflags(h)
    eq(hasbit(hhdr, 0x10), true, "header of the parent")
    eq(hasbit(hmain, 0x80), true, "flag of the parent")
    eq(hasbit(hchild, 0x80), true, "flag of the child")
    local g2 = loadstring(string.dump(g))
    eq(g2(3), 1, "the loaded dump runs")
    hdr, main = dumpflags(g2)
    eq(hasbit(hdr, 0x10), true, "header after a load")
  end)

  -- A function made once is specialized to its closure, and its immutable
  -- upvalues are constants in the trace, whether it uses bit operators or
  -- not.
  test("calls of a function with bit operators are specialized to the closure", function()
    local ok, util = pcall(require, "jit.util")
    if not (jit_on and ok) then return "skip" end
    local f = loadstring("local K = 15 return function(x) return x & K end")()
    jit.flush()
    local s = 0
    for i = 1, 100 do s = s + f(i) end
    eq(s, 730)
    -- Look for f among the constant operands of the traces' instructions.
    local found = false
    local function isf(tr, isref, op)
      return isref and op < 0 and rawequal(util.tracek(tr, op), f)
    end
    for tr = 1, 100 do
      local info = util.traceinfo(tr)
      for ins = 1, info and info.nins or 0 do
        local m, _, op1, op2 = util.traceir(tr, ins)
        if isf(tr, m % 4 == 0, op1) or isf(tr, math.floor(m / 4) % 4 == 0, op2) then
          found = true
        end
      end
    end
    eq(found, true, "a trace guards the closure itself")
  end)
end

print(string.format("# test_jit.lua (%s): %d passed, %d failed, %d skipped",
  jit and (jit.status() and "JIT on" or "JIT off") or "?", passed, failed, skipped))
os.exit(failed == 0 and 0 or 1)
