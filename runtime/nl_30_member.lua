-- Member access. On stock LuaJIT every member
-- read and member call of NL code goes through these helpers;
-- the fork replaces them with ISMETH and MSELF.
--
-- A member found through a receiver is a method when:
--   the receiver is a string, number or boolean (its type's library), or
--   the function is marked (I.methods), or
--   the receiver is a userdata whose metatable has a true `__methods`.
--
-- Intrinsics: rt.mget (x.m whose value is used), rt.mcall (x.m(args)),
-- rt.member (rt.member.m(x, args) is rt.mcall(x, "m", args)),
-- rt.mapply (rt.mapply(f, x, args): rt.mcall with the member `f` read
-- before, for `x.m?.(args)`),
-- rt.lmethod (rt.lmethod.m(x, args) is Lua's x:m(args) in NL code),
-- rt.pass (`return rt.pass(rt.mget(x, k))`: a return that is no tail call),
-- rt.index (x[k] and x.m! reads), rt.len (x.len), rt.primmethod, rt.Bool
-- (the table of Bool's methods).
do
  local methods = I.methods
  local is_method = I.is_method

  -- Methods defined on Bool. The table exists from the start;
  -- booleans get a metatable for Lua code when the first one is defined.
  I.Bool = {}
  -- `Self` as a value in `fn Bool.name`.
  rt.Bool = I.Bool

  -- ERROR POSITIONS. An error raised here must name the NL line that
  -- applied the intrinsic, so `level` counts frames from the raising
  -- function to the NL code: a runtime function that calls another one
  -- that raises does so in a non-tail call, and passes the level on.
  -- (Generated code calls the intrinsics outside tail position,
  -- crates/nl_luagen: LuaJIT keeps no frame for a tail call. A
  -- single-valued one in parentheses; any other as the argument of
  -- rt.pass, which returns all its arguments.)
  function rt.pass(...) return ... end

  -- The member `k` of `x`, and whether it is to be called as a method.
  -- An error names the frame `level` above the caller of lookup.
  local function lookup(x, k, level)
    local tx = type(x)
    if tx == "string" then
      return I.String[k], true
    elseif tx == "number" then
      return math[k], true
    elseif tx == "boolean" then
      return I.Bool[k], true
    elseif x == nil then
      error("attempt to read member '" .. tostring(k) .. "' of nil", level + 1)
    end
    local f = x[k]
    -- Only a function is called, so nothing else is asked for its mark:
    -- on the fork that question is a C function (host.vm.ismethod), and
    -- the JIT recorder ends a trace at every call of one.
    if type(f) ~= "function" then return f, false end
    if is_method(f) then return f, true end
    if tx == "userdata" then
      local mt = getmetatable(x)
      if type(mt) == "table" and rawget(mt, "__methods") then return f, true end
    end
    return f, false
  end
  I.lookup = lookup

  function rt.mget(x, k)
    local f, m = lookup(x, k, 2)
    if m and type(f) == "function" then return f(x) end
    return f
  end

  local function mcall(x, k, ...)
    local f, m = lookup(x, k, 2)
    if f == nil then
      error("attempt to call member '" .. tostring(k) .. "' of " .. rt.typeof(x) .. " (a nil value)", 2)
    end
    if m and type(f) == "function" then return f(x, ...) end
    return f(...)
  end
  rt.mcall = mcall

  -- `x.m?.(args)`: `f` is the member `m` of `x`, read once
  -- (with rt.index) and tested for nil by the generated code; it is called
  -- as rt.mcall would call it, without reading `m` again, so an `__index`
  -- runs once. The fork does this with MSELF.
  function rt.mapply(f, x, ...)
    local tx = type(x)
    if tx == "string" or tx == "number" or tx == "boolean" then return f(x, ...) end
    if type(f) == "function" then
      if is_method(f) then return f(x, ...) end
      if tx == "userdata" then
        local mt = getmetatable(x)
        if type(mt) == "table" and rawget(mt, "__methods") then return f(x, ...) end
      end
    end
    return f(...)
  end

  -- `rt.member.name(x, args)` is `rt.mcall(x, "name", args)`, one function
  -- per name. Generated code uses it for a member call whose name is a Lua
  -- identifier: LuaJIT names a function that raises "bad argument" after
  -- the key the caller read it with, so the message names the member
  -- (`'concat'`) and not a runtime helper.
  rt.member = setmetatable({}, {
    __index = function(t, k)
      local f = function(x, ...) return mcall(x, k, ...) end
      rawset(t, k, f)
      return f
    end,
  })

  -- `x:m(args)` in NL code, a Lua method call, also as a
  -- step of a Lua path: Lua's `x.m(x, args)`, except that a string
  -- receiver finds `m` in NL's `String` library, as `x.m` does (the
  -- file that contains the call decides which library answers).
  -- `rt.lmethod.m(x, args)`, one function per name, as for rt.member.
  local function lmcall(x, k, ...)
    local tx = type(x)
    if tx == "string" then return mcall(x, k, ...) end
    if tx ~= "table" then
      -- Indexing a value without an `__index` raises; raised here, it
      -- names the NL line.
      local mt = getmetatable(x)
      if type(mt) ~= "table" or rawget(mt, "__index") == nil then
        error("attempt to call method '" .. tostring(k) .. "' of a " .. rt.typeof(x) .. " value", 2)
      end
    end
    local f = x[k]
    if type(f) ~= "function" then
      local mt = getmetatable(f)
      if type(mt) ~= "table" or rawget(mt, "__call") == nil then
        error("attempt to call method '" .. tostring(k) .. "' (a " .. rt.typeof(f) .. " value)", 2)
      end
    end
    return f(x, ...)
  end
  rt.lmethod = setmetatable({}, {
    __index = function(t, k)
      local f = function(x, ...) return lmcall(x, k, ...) end
      rawset(t, k, f)
      return f
    end,
  })

  -- `x[k]` in NL code: a string receiver gets NL's library for a name and
  -- the character index for a number or a range.
  function rt.index(x, k)
    local tx = type(x)
    if tx == "string" then
      if type(k) == "string" then return I.String[k] end
      local v = I.strindex(x, k, 3)
      return v
    elseif tx == "boolean" then
      return I.Bool[k]
    elseif tx == "number" then
      return math[k]
    elseif x == nil then
      error("attempt to index a nil value with " .. (type(k) == "string" and ("'" .. k .. "'") or tostring(k)), 2)
    end
    return x[k]
  end

  -- `x.len`: Lua's `#`, or the `len` a struct defines. A
  -- value that has no length raises as Lua's `#` does, at the NL line.
  function rt.len(x)
    local tx = type(x)
    if tx == "table" then
      local mt = getmetatable(x)
      if mt ~= nil then
        local h = rawget(mt, "__len")
        if h ~= nil then return h(x) end
      end
    elseif tx ~= "string" then
      local mt = getmetatable(x)
      if type(mt) ~= "table" or rawget(mt, "__len") == nil then
        error("attempt to get the length of a " .. rt.typeof(x) .. " value", 2)
      end
    end
    return #x
  end

  -- `fn Number.name`, `fn String.name`, `fn Bool.name`.
  -- Redefining a method that exists and came from a different file is an
  -- error; the first definition stays. The error has no type
  -- of its own; it is a plain Exception.
  --
  -- `file` is the owner of the defining chunk (rt.owner, nl_70_module.lua):
  -- a file's canonical name, or a value of its own for a chunk that is not
  -- a file.
  local owners = {}
  local targets = { Number = math, Bool = I.Bool }
  -- The message of the error that defining `tname.name` from `file` would
  -- raise, or nil when the definition is allowed.
  function I.primmethod_refusal(tname, name, file)
    local target = targets[tname] or I.String
    local key = tname .. "." .. name
    if target[name] ~= nil and owners[key] ~= file then
      return "cannot redefine " .. key .. ": it is already defined by " .. tostring(owners[key] or "the standard library")
    end
    return nil
  end
  function rt.primmethod(tname, name, f, file)
    local target = targets[tname] or I.String
    local key = tname .. "." .. name
    local refusal = I.primmethod_refusal(tname, name, file)
    if refusal ~= nil then I.throw("Exception", refusal, 2) end
    owners[key] = file
    I.mark(f)
    target[name] = f
    if tname == "Bool" and I.setmetatable_bool then I.setmetatable_bool() end
  end
end
