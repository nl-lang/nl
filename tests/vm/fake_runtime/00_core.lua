-- Fake NL runtime for the VM tests (tests/vm). It implements the host
-- interface of runtime/nl.lua (header comment) just far enough to test the
-- C API functions that call it. It is not the NL runtime: the real one is
-- written in runtime/.
--
-- The files of this directory are concatenated in name order into one chunk,
-- as the real runtime's are: this file opens the chunk, 99_return.lua ends it.

local host = ...
local rt = {}
rt.fake = true
rt.event_was_c = type(event) == "table" and type(event.on_run) == "function"
  and pcall(string.dump, event.on_run) == false   -- C functions cannot be dumped

-- Types ---------------------------------------------------------------------

local types = {}          -- name -> type
local hosttypes = {}      -- metatable -> type
local INST = {}           -- key of an instance's type in its metatable

local function newtype(name, parent, kind, defaults)
  local T = { name = name, parent = parent, kind = kind, defaults = defaults or {} }
  T.instmt = { [INST] = T,
    __tostring = function(v) return name .. ": " .. tostring(rawget(v, "message")) end }
  T.instmt.__index = T.instmt
  types[name] = T
  return T
end

local function typeofvalue(v)
  local t = type(v)
  if t == "table" or t == "userdata" then
    local mt = getmetatable(v)
    if mt ~= nil then
      if hosttypes[mt] then return hosttypes[mt] end
      if type(mt) == "table" and rawget(mt, INST) then return rawget(mt, INST) end
    end
  end
  return nil
end

local PRIM = { number = "Number", string = "String", boolean = "Bool",
  table = "Table", ["function"] = "Function", ["nil"] = "Nil",
  userdata = "Userdata", thread = "Thread" }

function rt.typeof(v)
  local T = typeofvalue(v)
  if T then return T.name end
  return PRIM[type(v)]
end

function rt.isa(v, T)
  local VT = typeofvalue(v)
  while VT do
    if VT == T then return true end
    VT = VT.parent
  end
  return false
end

function rt.gettype(name) return types[name] end

function rt.settype(mt, name, parent_mt)
  local old = types[name]
  if old and old.kind == "host" and old.mt ~= mt then
    error("the host type name '" .. name .. "' is already registered", 2)
  end
  local parent = nil
  if parent_mt ~= nil then
    parent = hosttypes[parent_mt]
    if not parent then error("the parent is not a host type", 2) end
  end
  local T = newtype(name, parent, "host")
  T.mt = mt
  hosttypes[mt] = T
end

function rt.new(T, fields)
  if type(T) ~= "table" or T.kind ~= "struct" then error("not a struct type", 2) end
  local inst = {}
  for k, v in pairs(T.defaults) do inst[k] = v end
  for k, v in pairs(fields) do inst[k] = v end
  return setmetatable(inst, T.instmt)
end

-- Ranges --------------------------------------------------------------------

local RANGE = {}
RANGE.__index = RANGE
local RangeT = newtype("Range", nil, "builtin")
function rt.range(first, last, exclusive)
  return setmetatable({ first = first, last = last, exclusive = exclusive and true or false }, RANGE)
end
function rt.torange(v)
  if getmetatable(v) == RANGE then return v.first, v.last, v.exclusive end
end

-- Exceptions ----------------------------------------------------------------

local Exception = newtype("Exception", nil, "struct", { message = "" })
local LuaError = newtype("LuaError", Exception, "struct", { message = "" })
local TypeError = newtype("TypeError", Exception, "struct", { message = "" })
newtype("MatchError", Exception, "struct", { message = "" })

local function isexception(v) return rt.isa(v, Exception) end
rt.isexception = isexception

function rt.exception(typename, message)
  local T = types[typename or "Exception"]
  if not T or not rt.isa(setmetatable({}, T.instmt), Exception) then
    error("unknown exception type '" .. tostring(typename) .. "'", 2)
  end
  return rt.new(T, { message = message })
end

local translator = nil
function rt.settranslator(f) translator = f end

function rt.handler(e)
  if not isexception(e) then
    local t = translator and translator(e)
    if t ~= nil then e = t
    else e = rt.new(LuaError, { message = tostring(e), value = e }) end
  end
  if rawget(e, "trace") == nil then rawset(e, "trace", debug.traceback("", 2)) end
  return e
end

function rt.raise(v)
  if type(v) == "string" then v = rt.new(Exception, { message = v }) end
  error(v, 0)
end

function rt.typeerror(narg, expected, v)
  local name = type(expected) == "table" and expected.name or tostring(expected)
  rt.raise(rt.new(TypeError, { message = "bad argument #" .. narg .. ": expected "
    .. name .. ", got " .. rt.typeof(v) }))
end

function rt.tostring(v)
  if isexception(v) then return typeofvalue(v).name .. ": " .. tostring(v.message) end
  return tostring(v)
end
