-- A hand assembler for LuaJIT 2.1 bytecode dumps (LuaJIT/src/lj_bcdump.h,
-- lj_bcwrite.c), in Lua, so that tests can build prototypes that use the
-- NL opcodes of the fork (lj_bc.h, BCDEF_NL) without the compiler.
--
--   local asm = require("bcasm")
--   local f = asm.load(asm.dump(asm.proto{
--     numparams = 0, framesize = 1,
--     bc = { {"KSHORT", 0, 7}, {"RET1", 0, 2} },
--   }))
--   assert(f() == 7)
--
-- A proto description:
--   numparams, framesize   bytes
--   vararg = true          PROTO_VARARG
--   method = true          BCDUMP_PF_METHOD (closures are NL methods)
--   nl = true              BCDUMP_PF_NL (compiled from NL)
--   uvdata = { ... }       one 16-bit word per upvalue: asm.UV_LOCAL + slot
--                          for an enclosing local, or the enclosing
--                          function's upvalue index
--   kgc = { ... }          GC constants, index 0 first: a string, or a child
--                          proto (from asm.proto)
--   knum = { ... }         number constants, index 0 first
--   bc = { ... }           instructions {"OP", A, D} or {"OP", A, B, C}, in
--                          order, without the FUNCF/FUNCV header. A string
--                          "name:" defines a label; a jump operand D may be
--                          a label name or {to = pc}: the assembler encodes
--                          the relative jump. KSHORT takes a signed literal.
--
-- asm.dump(main [, opts]) returns the dump string. opts.flags replaces the
-- header flags (STRIP|FR2, plus NL when a proto needs it); opts.nl forces
-- BCDUMP_F_NL on or off.

local M = {}

-- Opcode numbers: the BCDEF table of lj_bc.h, with BCDEF_NL between BSAR
-- and the function headers. M.check_vmdef compares them with the built
-- interpreter's jit/vmdef.lua.
M.opnames = {
  "ISLT", "ISGE", "ISLE", "ISGT", "ISEQV", "ISNEV", "ISEQS", "ISNES", "ISEQN",
  "ISNEN", "ISEQP", "ISNEP", "ISTC", "ISFC", "IST", "ISF", "ISTYPE", "ISNUM",
  "MOV", "NOT", "UNM", "LEN", "ADDVN", "SUBVN", "MULVN", "DIVVN", "MODVN",
  "ADDNV", "SUBNV", "MULNV", "DIVNV", "MODNV", "ADDVV", "SUBVV", "MULVV",
  "DIVVV", "MODVV", "POW", "CAT", "KSTR", "KCDATA", "KSHORT", "KNUM", "KPRI",
  "KNIL", "UGET", "USETV", "USETS", "USETN", "USETP", "UCLO", "FNEW", "TNEW",
  "TDUP", "GGET", "GSET", "TGETV", "TGETS", "TGETB", "TGETR", "TSETV", "TSETS",
  "TSETB", "TSETM", "TSETR", "CALLM", "CALL", "CALLMT", "CALLT", "ITERC",
  "ITERN", "VARG", "ISNEXT", "RETM", "RET", "RET0", "RET1", "FORI", "JFORI",
  "FORL", "IFORL", "JFORL", "ITERL", "IITERL", "JITERL", "LOOP", "ILOOP",
  "JLOOP", "JMP", "BNOT", "BAND", "BOR", "BXOR", "BSHL", "BSHR", "BSAR",
  -- NL extensions (BCDEF_NL)
  "CMPGT", "CMPNGT", "CMPGE", "CMPNGE", "KINTR", "ISMETH", "MSELF",
  -- function headers (never in a dump)
  "FUNCF", "IFUNCF", "JFUNCF", "FUNCV", "IFUNCV", "JFUNCV", "FUNCC", "FUNCCW",
}
M.ops = {}
for i, name in ipairs(M.opnames) do M.ops[name] = i - 1 end
assert(M.ops.CMPGT == 96 and M.ops.MSELF == 102 and M.ops.FUNCF == 103)

-- Instructions whose D operand is a jump target.
local jumpops = {
  JMP = true, UCLO = true, ISNEXT = true, FORI = true, JFORI = true,
  FORL = true, IFORL = true, ITERL = true, IITERL = true, LOOP = true,
  ILOOP = true,
}
local nlops = {
  CMPGT = true, CMPNGT = true, CMPGE = true, CMPNGE = true, KINTR = true,
  ISMETH = true, MSELF = true,
}

M.F_BE, M.F_STRIP, M.F_FFI, M.F_FR2, M.F_BITOP, M.F_NL = 0x01, 0x02, 0x04, 0x08, 0x10, 0x20
M.PROTO_CHILD, M.PROTO_VARARG, M.PF_METHOD, M.PF_NL = 0x01, 0x02, 0x08, 0x10
M.UV_LOCAL, M.UV_IMMUTABLE = 0x8000, 0x4000
M.KGC_CHILD, M.KGC_STR = 0, 5
M.PRI = { ["nil"] = 0, ["false"] = 1, ["true"] = 2 }

-- -- Byte writers -----------------------------------------------------------

local function uleb128(buf, v)
  assert(v >= 0 and v == math.floor(v), "uleb128 of a bad value")
  while v >= 0x80 do
    buf[#buf + 1] = string.char(v % 0x80 + 0x80)
    v = math.floor(v / 0x80)
  end
  buf[#buf + 1] = string.char(v)
end

local function w32(buf, v)
  assert(v >= 0 and v < 2^32)
  buf[#buf + 1] = string.char(v % 256, math.floor(v / 256) % 256,
                              math.floor(v / 65536) % 256, math.floor(v / 2^24) % 256)
end

local function w16(buf, v)
  assert(v >= 0 and v < 65536)
  buf[#buf + 1] = string.char(v % 256, math.floor(v / 256))
end

-- The 33-bit ULEB128 of lj_bcread.c's bcread_uleb128_33: bit 0 of the first
-- byte is `flag`, bits 1-6 the low 6 bits of `v`, and when v >= 0x40 the
-- rest of v follows as an ordinary ULEB128 (7 bits per byte).
local function uleb128_33(buf, flag, v)
  assert(v >= 0 and v < 2^32 and v == math.floor(v))
  local lo = v % 0x40
  local rest = math.floor(v / 0x40)
  if rest == 0 then
    buf[#buf + 1] = string.char(flag + 2 * lo)
  else
    buf[#buf + 1] = string.char(flag + 2 * lo + 0x80)
    uleb128(buf, rest)
  end
end

-- The two 32-bit words of a double (little endian: lo first).
local function doublewords(x)
  if x ~= x then return 0, 0x7FF80000 end  -- a quiet NaN
  local sign = 0
  if x < 0 or (x == 0 and 1 / x < 0) then sign = 0x80000000; x = -x end
  if x == math.huge then return 0, sign + 0x7FF00000 end
  if x == 0 then return 0, sign end
  local m, e = math.frexp(x)  -- x = m * 2^e, 0.5 <= m < 1
  local biased = e + 1022
  local frac
  if biased <= 0 then  -- subnormal: x = 0.frac * 2^-1022
    frac = x * 2^1074  -- an integer < 2^52
    biased = 0
  else
    assert(biased < 0x7FF, "double out of range")
    frac = (m * 2 - 1) * 2^52
  end
  local hi = math.floor(frac / 2^32)
  local lo = frac - hi * 2^32
  return lo, sign + biased * 2^20 + hi
end

-- One number constant (lj_bcwrite.c bcwrite_knum / lj_bcread.c
-- bcread_knum): an integer as the 33-bit ULEB with flag 0; anything else
-- as the low word with flag 1 then the high word as a plain ULEB128.
local function knum(buf, n)
  assert(type(n) == "number", "knum: not a number")
  if n == math.floor(n) and n >= -2^31 and n < 2^31 and not (n == 0 and 1 / n < 0) then
    local u = n < 0 and n + 2^32 or n
    uleb128_33(buf, 0, u)
  else
    local lo, hi = doublewords(n)
    uleb128_33(buf, 1, lo)
    uleb128(buf, hi)
  end
end
M.knumbytes = function(n) local b = {}; knum(b, n); return table.concat(b) end

-- -- Prototypes -------------------------------------------------------------

local protometa = { __index = {} }

function M.proto(p)
  assert(type(p.framesize) == "number", "proto: framesize required")
  p.numparams = p.numparams or 0
  p.uvdata = p.uvdata or {}
  p.kgc = p.kgc or {}
  p.knum = p.knum or {}
  assert(type(p.bc) == "table" and #p.bc > 0, "proto: bc required")
  return setmetatable(p, protometa)
end

local function isproto(x) return getmetatable(x) == protometa end

-- Encodes the instruction list: resolves labels and jumps, returns the list
-- of 32-bit words and whether an NL opcode occurs.
local function encode_bc(bc)
  local labels, insns = {}, {}
  for _, ins in ipairs(bc) do
    if type(ins) == "string" then
      local name = ins:match("^(%w+):$")
      assert(name, "bad label " .. ins)
      assert(not labels[name], "duplicate label " .. name)
      labels[name] = #insns
    else
      insns[#insns + 1] = ins
    end
  end
  local words, usesnl = {}, false
  for pc, ins in ipairs(insns) do
    pc = pc - 1
    local opname = ins[1]
    local op = M.ops[opname]
    assert(op, "unknown opcode " .. tostring(opname))
    assert(op < M.ops.FUNCF, "function headers are not written: " .. opname)
    if nlops[opname] then usesnl = true end
    local a = ins[2] or 0
    assert(a >= 0 and a < 256, opname .. ": A out of range")
    local d
    if #ins == 4 then
      local b, c = ins[3], ins[4]
      assert(b >= 0 and b < 256 and c >= 0 and c < 256, opname .. ": B/C out of range")
      d = b * 256 + c
    else
      d = ins[3] or 0
      if jumpops[opname] then
        local target
        if type(d) == "string" then
          target = labels[d]; assert(target, "unknown label " .. d)
        elseif type(d) == "table" then
          target = d.to
        else
          target = pc + 1 + d  -- a relative offset
        end
        assert(target >= 0 and target <= #insns, opname .. ": jump target out of range")
        d = target - (pc + 1) + 0x8000
      elseif opname == "KSHORT" and d < 0 then
        d = d + 0x10000
      end
      assert(d >= 0 and d < 65536, opname .. ": D out of range")
    end
    words[#words + 1] = op + a * 256 + d * 65536
  end
  return words, usesnl
end

-- Whether a proto, or one of its children, needs BCDUMP_F_NL.
local function needsnl(p)
  if p.method or p.nl then return true end
  local _, usesnl = encode_bc(p.bc)
  if usesnl then return true end
  for _, k in ipairs(p.kgc) do
    if isproto(k) and needsnl(k) then return true end
  end
  return false
end
M.needsnl = needsnl

-- Writes one proto (children first) to the dump buffer.
local function write_proto(out, p)
  local haschild = false
  for _, k in ipairs(p.kgc) do
    if isproto(k) then haschild = true; write_proto(out, k) end
  end
  local words = encode_bc(p.bc)
  local body = {}
  local flags = (haschild and M.PROTO_CHILD or 0) + (p.vararg and M.PROTO_VARARG or 0)
                + (p.method and M.PF_METHOD or 0) + (p.nl and M.PF_NL or 0)
  body[#body + 1] = string.char(flags, p.numparams, p.framesize, #p.uvdata)
  uleb128(body, #p.kgc)
  uleb128(body, #p.knum)
  uleb128(body, #words)
  for _, w in ipairs(words) do w32(body, w) end
  for _, uv in ipairs(p.uvdata) do w16(body, uv) end
  for i = #p.kgc, 1, -1 do  -- highest index first (lj_bcwrite.c bcwrite_kgc)
    local k = p.kgc[i]
    if isproto(k) then
      uleb128(body, M.KGC_CHILD)
    elseif type(k) == "string" then
      uleb128(body, M.KGC_STR + #k)
      body[#body + 1] = k
    else
      error("kgc: unsupported constant " .. tostring(k))
    end
  end
  for _, n in ipairs(p.knum) do knum(body, n) end
  local pdata = table.concat(body)
  uleb128(out, #pdata)
  out[#out + 1] = pdata
end

function M.dump(main, opts)
  opts = opts or {}
  assert(isproto(main), "dump: not a proto")
  local out = { "\27LJ", string.char(2) }
  local flags = opts.flags
  if not flags then
    flags = M.F_STRIP + M.F_FR2
    local nl = opts.nl
    if nl == nil then nl = needsnl(main) end
    if nl then flags = flags + M.F_NL end
  end
  uleb128(out, flags)
  if flags % (2 * M.F_STRIP) < M.F_STRIP then  -- not stripped: a chunk name
    local name = opts.name or "=bcasm"
    uleb128(out, #name)
    out[#out + 1] = name
  end
  write_proto(out, main)
  out[#out + 1] = "\0"
  return table.concat(out)
end

-- Loads a dump; errors with the loader's message.
function M.load(dump, name)
  local f, err = (loadstring or load)(dump, name or "=bcasm")
  if not f then error(err, 2) end
  return f
end

-- Assembles and loads in one step.
function M.fn(p, opts) return M.load(M.dump(M.proto(p), opts)) end

-- Compares M.opnames with the interpreter's jit/vmdef.lua (built next to the
-- luajit binary, arg[-1]); returns true, or nil and a reason when the file
-- cannot be found, or raises on a mismatch.
function M.check_vmdef(path)
  local vmdef
  if path then
    local f = loadfile(path); if not f then return nil, "no " .. path end
    vmdef = f()
  else
    local ok, m = pcall(require, "jit.vmdef")
    if ok then vmdef = m
    else
      local exe  -- the interpreter: the lowest negative index of arg
      if arg then local i = -1; while arg[i] do exe = arg[i]; i = i - 1 end end
      local dir = exe and exe:match("^(.*)/") or "."
      local f = loadfile(dir .. "/jit/vmdef.lua")
      if not f then return nil, "no jit/vmdef.lua next to the interpreter" end
      vmdef = f()
    end
  end
  local bn = vmdef.bcnames
  local names = {}
  for i = 1, #bn, 6 do names[#names + 1] = (bn:sub(i, i + 5):gsub(" ", "")) end
  if #names ~= #M.opnames then
    error(("opcode count: assembler %d, vmdef %d"):format(#M.opnames, #names))
  end
  for i, name in ipairs(names) do
    if M.opnames[i] ~= name then
      error(("opcode %d: assembler %s, vmdef %s"):format(i - 1, M.opnames[i], name))
    end
  end
  return true
end

return M
