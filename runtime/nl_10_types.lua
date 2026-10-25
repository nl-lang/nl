-- Types: descriptors, runtime checks, `is`, `typeof`, host types.
--
-- A type value is one of:
--   a descriptor { name = "Number", test = function(v) ... end }: Lua's types
--     (rt.T), and the combinations made by rt.optional, rt.union,
--     rt.negate, rt.record, rt.lazy;
--   a struct, enum or interface table (I.info has its description);
--   a built-in type that is not a struct (`Range`, `Task`);
--   a host-type metatable registered with rt.settype.
--
-- Intrinsics: rt.T, rt.check, rt.ret, rt.isa, rt.optional, rt.union,
-- rt.negate, rt.record, rt.lazy, rt.typeof, rt.typename; rt.type,
-- rt.getmetatable, rt.setmetatable (for inline checks).
--
-- `getmetatable` in these files is the runtime's local of nl.lua, the raw
-- `debug.getmetatable` where the host has it, not the global: a class or
-- host type whose metatable sets `__metatable` is still recognized by
-- its metatable (rt.luaclass, the built-in and host-type tests).
do
  local T = {}
  rt.T = T

  -- What the runtime knows about structs, enums, interfaces, modules,
  -- host types and the built-in types, keyed by their table:
  -- I.info[t] = { kind = "struct", name = ..., ... }.
  --
  -- A description must not keep its type alive: a weak-keyed table whose
  -- value refers to its key keeps the key forever, since LuaJIT has no
  -- ephemerons. A struct's description refers to the struct (its
  -- ancestors, the closures of its field types and defaults, its operator
  -- definitions and interface methods), so it is kept in the struct's own
  -- metatable, under a private key: it lives exactly as long as the
  -- struct. The descriptions of the other kinds do not refer to their
  -- type and are kept in a weak-keyed table. I.structs is the weak set of
  -- every struct, for the code that walks them (nl_42_opdef.lua).
  local INFO = {}
  local other_info = setmetatable({}, { __mode = "k" })
  -- The transfer encoder reads it (nl_64_worker.lua).
  I.other_info = other_info
  local structs = setmetatable({}, { __mode = "k" })
  I.structs = structs
  I.info = setmetatable({}, {
    -- On the path of every operator a struct defines and of every type
    -- test, so: `rawget` on other_info, which has a metatable (for
    -- `__mode`), where a plain read that finds nothing looks for
    -- `__index` each time, in a trace too; and no tail call at the end,
    -- since the JIT recorder counts every tail call toward its
    -- loop-unroll limit (see `numeric` in nl_40_ops.lua), and a loop body
    -- reads many descriptions.
    __index = function(_, t)
      if type(t) ~= "table" then return nil end
      local info = rawget(other_info, t)
      if info ~= nil then return info end
      local m = getmetatable(t)
      if type(m) == "table" then
        info = rawget(m, INFO)
        return info
      end
      return nil
    end,
    __newindex = function(_, t, info)
      if type(t) ~= "table" then error("NL runtime: only a table can be a type", 2) end
      if info ~= nil and info.kind == "struct" then
        local m = getmetatable(t)
        if m == nil then
          m = {}
          setmetatable(t, m)
        end
        rawset(m, INFO, info)
        structs[t] = true
      else
        other_info[t] = info
      end
    end,
  })

  -- An incomplete type (I.PARTIAL set): a combination with a part that was
  -- nil when it was made, because it names a type declared later in the
  -- file. rt.new does not cache such a field type (nl_20_struct.lua). A
  -- union whose last part is nil cannot be told from a shorter one, so
  -- rt.new also evaluates a cached type again before it reports a
  -- mismatch.
  local PARTIAL = {}
  I.PARTIAL = PARTIAL

  -- What an optional and a union are made of, kept in the descriptor
  -- under these keys (`T?`: the type `T`; `A | B`: the list of parts), for
  -- the constructors of nl_20_struct.lua, which test such a field type
  -- inline.
  local OPTIONAL, UNION = {}, {}
  I.OPTIONAL, I.UNION = OPTIONAL, UNION

  local function prim(name, luatype)
    T[name] = { name = name, test = function(v) return type(v) == luatype end }
  end
  prim("Number", "number")
  prim("String", "string")
  prim("Bool", "boolean")
  prim("Table", "table")
  prim("Function", "function")
  prim("Userdata", "userdata")
  prim("Thread", "thread")
  prim("Nil", "nil")
  T.Nil.name = "nil"
  T.Any = { name = "any", test = function() return true end }

  local isa

  -- The NL name of a type value, for messages.
  local function typename(t)
    if type(t) ~= "table" then return tostring(t) end
    local info = I.info[t]
    if info then return info.name end
    local name = rawget(t, "name")
    if type(name) == "string" then return name end
    return "?"
  end
  rt.typename = typename

  -- nl_typename: the name of a type value, nil for a value that is not
  -- one (a module, an instance, any other table).
  function rt.typenameof(t)
    if type(t) ~= "table" then return nil end
    local info = I.info[t]
    if info then
      if I.isa_kind[info.kind] == nil then return nil end
      return info.name
    end
    local name = rawget(t, "name")
    if type(name) == "string" and type(rawget(t, "test")) == "function" then return name end
    return nil
  end

  isa = function(v, t)
    local info = I.info[t]
    if info then
      local check = I.isa_kind[info.kind]
      if check then
        local r = check(v, t, info) -- not a tail call (see I.info)
        return r
      end
      return false
    end
    if type(t) == "table" and t.test then return t.test(v) == true end
    return false
  end
  rt.isa = isa

  -- Membership tests by kind of type; the struct, enum and interface files
  -- add theirs.
  I.isa_kind = {}

  -- The built-in types that are not structs: a value is one when its
  -- metatable is the type.
  I.isa_kind.builtin = function(v, t)
    local tv = type(v)
    return (tv == "table" or tv == "userdata") and getmetatable(v) == t
  end

  -- A part of a combination, named for a message; `wrap` puts parentheses
  -- around a union or a negation, so that `(A | B)?` reads as it is.
  local function part_name(t, wrap)
    if t == nil then return "<not yet defined>" end
    local name = typename(t)
    if wrap and type(t) == "table" and rawget(t, "composite") then return "(" .. name .. ")" end
    return name
  end

  local function is_partial(t)
    return t == nil or (type(t) == "table" and rawget(t, PARTIAL) == true)
  end

  function rt.optional(t)
    return {
      name = part_name(t, true) .. "?",
      test = function(v) return v == nil or isa(v, t) end,
      [PARTIAL] = is_partial(t) or nil,
      [OPTIONAL] = t,
    }
  end

  -- The tests of a union and of a record are written out for up to four
  -- parts: the JIT recorder gives up on a loop inside the function it
  -- records as soon as the loop comes round ("inner loop in root trace"),
  -- so a loop over the parts kept a function with such a parameter out of
  -- a trace whenever the value was not the first part, or the record had
  -- two fields. The parts are read and tested in the loop's order.
  function rt.union(list)
    local names, partial = {}, false
    local n = #list
    for i = 1, n do
      names[i] = part_name(list[i], false)
      partial = partial or is_partial(list[i])
    end
    local test
    if n <= 4 then
      test = function(v)
        if n >= 1 and isa(v, list[1]) then return true end
        if n >= 2 and isa(v, list[2]) then return true end
        if n >= 3 and isa(v, list[3]) then return true end
        if n >= 4 and isa(v, list[4]) then return true end
        return false
      end
    else
      test = function(v)
        for i = 1, n do
          if isa(v, list[i]) then return true end
        end
        return false
      end
    end
    return {
      name = tconcat(names, " | "),
      composite = true,
      test = test,
      [PARTIAL] = partial or nil,
      [UNION] = list,
    }
  end

  function rt.negate(t)
    return {
      name = "~" .. part_name(t, true),
      composite = true,
      test = function(v) return not isa(v, t) end,
      [PARTIAL] = is_partial(t) or nil,
    }
  end

  -- A record: a table with at least these fields, of these
  -- types, checked field by field.
  function rt.record(fields)
    local parts, partial = {}, false
    -- The field names in the order `pairs` gives them, which stays the
    -- same: nothing changes `fields` once the type is made.
    local keys, n = {}, 0
    for k, ft in pairs(fields) do
      parts[#parts + 1] = tostring(k) .. ": " .. part_name(ft, false)
      partial = partial or is_partial(ft)
      n = n + 1
      keys[n] = k
    end
    tsort(parts)
    local test
    if n <= 4 then
      -- Written out, as for a union (see rt.union).
      local k1, k2, k3, k4 = keys[1], keys[2], keys[3], keys[4]
      test = function(v)
        if type(v) ~= "table" then return false end
        if n >= 1 and not isa(v[k1], fields[k1]) then return false end
        if n >= 2 and not isa(v[k2], fields[k2]) then return false end
        if n >= 3 and not isa(v[k3], fields[k3]) then return false end
        if n >= 4 and not isa(v[k4], fields[k4]) then return false end
        return true
      end
    else
      test = function(v)
        if type(v) ~= "table" then return false end
        for k, ft in pairs(fields) do
          if not isa(v[k], ft) then return false end
        end
        return true
      end
    end
    return {
      name = "{" .. tconcat(parts, ", ") .. "}",
      test = test,
      [PARTIAL] = partial or nil,
    }
  end

  -- A type an alias refers to before it is built, as in a recursive
  -- alias (`type Node = {next: Node?}`): `get()` returns it, and is asked
  -- the first time a value is tested.
  function rt.lazy(name, get)
    local t
    return {
      name = name,
      test = function(v)
        if t == nil then t = get() end
        return isa(v, t)
      end,
    }
  end

  -- `declare struct Name = :path`, a Lua class: a value is one when its
  -- metatable is the class table `get()` reads from the path. A class
  -- that does not exist yet has no instances, and is looked for again at
  -- the next test. With `keep` (a path that makes a call, such as gmod's
  -- `FindMetaTable("Color")`, a C function that ends a trace) the class
  -- found first is kept in the field `class`, which generated code tests
  -- inline (crates/nl_ir, lower/types.rs); a path of fields is read at
  -- every test, so a class table replaced later is seen. Generated code
  -- builds one per type and file, in the file's type pool.
  function rt.luaclass(name, get, keep)
    local t = { name = name }
    t.test = function(v)
      local c = t.class
      if c == nil then
        c = get()
        if c == nil then return false end
        if keep then t.class = c end
      end
      return getmetatable(v) == c
    end
    return t
  end

  local lua_names = {
    number = "Number", string = "String", boolean = "Bool", table = "Table",
    ["function"] = "Function", userdata = "Userdata", thread = "Thread",
    -- `typeof(nil)` is "nil", the type's name.
    ["nil"] = "nil",
    -- LuaJIT's FFI values (`ffi.new`, `1LL`): Lua's type in NL
    -- spelling, and nothing more.
    cdata = "Cdata",
  }

  -- The struct name for struct instances, the host type name
  -- for userdata of a host type, otherwise Lua's type in NL spelling.
  -- Ranges, tasks and regexes report "Range", "Task" and "Regex"
  -- (I.typeof_special, set by their files). Enum values are their
  -- underlying strings or numbers. Never nil: a type Lua adds later is
  -- reported by Lua's own name.
  I.typeof_special = setmetatable({}, { __mode = "k" })
  function rt.typeof(v)
    local tv = type(v)
    if tv == "table" or tv == "userdata" then
      local mt = getmetatable(v)
      if mt ~= nil then
        local special = I.typeof_special[mt]
        if special then return special end
        local info = I.info[mt]
        if info and (info.kind == "struct" or info.kind == "host") then return info.name end
      end
    end
    return lua_names[tv] or tv
  end

  -- TypeError: "<what> expects <type>, got <type>".
  local function check_failed(v, t, what)
    local msg = tostring(what) .. " expects " .. typename(t) .. ", got " .. rt.typeof(v)
    I.throw("TypeError", msg)
  end

  function rt.check(v, t, what)
    if not isa(v, t) then check_failed(v, t, what) end
  end

  -- What generated code tests inline before it calls rt.check (crates/
  -- nl_ir, lower/types.rs): `rt.type(x) == "number"` for `Number`,
  -- `rt.getmetatable(x) == S` for a struct, and so on. Each test is
  -- sufficient, not necessary: when it fails, rt.check applies the whole
  -- rule (a descendant of S passes) and builds the message. The JIT
  -- compiles all three. rt.getmetatable is the runtime's own, the raw one
  -- where the debug library has it (nl.lua), the one the struct test
  -- reads (I.isa_kind.struct), so the two agree whatever `__metatable`
  -- says. A struct literal ends with rt.setmetatable
  -- (nl_20_struct.lua, rt.newfield).
  rt.type = type
  rt.getmetatable = getmetatable
  rt.setmetatable = setmetatable

  -- A checked return: the first value is checked, all are returned.
  function rt.ret(t, what, ...)
    local v = ...
    if not isa(v, t) then check_failed(v, t, what) end
    return ...
  end

  -- A `(A, B, …)` return type: value `i` is checked against
  -- type `i`, a missing value is `nil`, and exactly `n` values are
  -- returned, so a tail call that gives more is cut to the declared count.
  -- `what` is a format with one `%d`, the value's number.
  local function retn_tail(types, n, what, i, ...)
    if i > n then return end
    local v = ...
    if not isa(v, types[i]) then check_failed(v, types[i], sformat(what, i)) end
    return v, retn_tail(types, n, what, i + 1, select(2, ...))
  end
  function rt.retn(types, n, what, ...)
    return retn_tail(types, n, what, 1, ...)
  end

  -- nlL_typeerror / nlL_checkisa: `narg` is an argument number or a
  -- description; `expected` a type name or a type.
  function rt.typeerror(narg, expected, v)
    local what = type(narg) == "number" and ("argument " .. narg) or tostring(narg)
    local name = type(expected) == "string" and expected or typename(expected)
    I.throw("TypeError", what .. " expects " .. name .. ", got " .. rt.typeof(v))
  end

  -- Host types: a userdata metatable with an NL
  -- name and an optional parent. A name belongs to one metatable.
  local host_types = {}

  -- The state-wide names that are not host types: Exception
  -- and its built-in descendants, Range, Task, Regex, Time and Duration.
  -- Their values are set by the files that define them.
  local builtin_names = {
    Exception = true, LuaError = true, TypeError = true, MatchError = true, ConversionError = true,
    RegexError = true, EncodingError = true, KeyError = true, SendError = true, WorkerError = true,
    Range = true, Task = true, Regex = true, Time = true, Duration = true,
  }
  local function builtin_type(name)
    if name == "Range" then return I.Range end
    return rt[name]
  end

  -- Unspecified: a host type cannot take the name of a
  -- built-in type, nor a name `typeof` gives to one of Lua's types, since
  -- `typeof` and the checks could not tell them apart.
  local reserved = {}
  for name in pairs(builtin_names) do reserved[name] = true end
  for _, name in pairs(lua_names) do reserved[name] = true end
  reserved.Bool, reserved.Any, reserved.Nil = true, true, true

  function rt.settype(mt, name, parent_mt)
    if type(mt) ~= "table" then
      error("settype: the metatable of a host type must be a table, not " .. rt.typeof(mt), 2)
    end
    if type(name) ~= "string" or not smatch(name, "^[%a_][%w_]*$") then
      error("settype: the name of a host type must be an identifier, not "
        .. (type(name) == "string" and ("'" .. name .. "'") or rt.typeof(name)), 2)
    end
    if reserved[name] then
      error("settype: '" .. name .. "' is the name of a built-in type", 2)
    end
    local prev = host_types[name]
    if prev ~= nil and prev ~= mt then
      error("settype: the host type name '" .. name .. "' is already registered for another metatable", 2)
    end
    local info = I.info[mt]
    if info ~= nil and (info.kind ~= "host" or info.name ~= name) then
      error("settype: this metatable is already the type " .. tostring(info.name), 2)
    end
    if parent_mt ~= nil then
      local pinfo = type(parent_mt) == "table" and other_info[parent_mt] or nil
      if pinfo == nil or pinfo.kind ~= "host" then
        error("settype: the parent of host type '" .. name .. "' must be the metatable of a host type", 2)
      end
      local p, guard = parent_mt, 0
      while p ~= nil and guard < 64 do
        if p == mt then error("settype: host type '" .. name .. "' cannot be its own ancestor", 2) end
        local i = other_info[p]
        p = i and i.parent
        guard = guard + 1
      end
    end
    other_info[mt] = { kind = "host", name = name, parent = parent_mt }
    host_types[name] = mt
  end

  -- nl_gettype: a host type or one of the built-in names, else nil.
  function rt.gettype(name)
    if type(name) ~= "string" then return nil end
    if builtin_names[name] then return builtin_type(name) end
    return host_types[name]
  end

  I.isa_kind.host = function(v, t)
    if type(v) ~= "userdata" then return false end
    local mt = getmetatable(v)
    if mt == nil then return false end
    -- At most 64 types, as a counted loop that is left before its end
    -- when the value's own type is `t` or has no parent: the JIT
    -- recorder gives up on a loop inside the function it records only
    -- when the loop comes round ("inner loop in root trace").
    for _ = 1, 64 do
      if mt == t then return true end
      local info = other_info[mt]
      mt = info and info.parent
      if mt == nil then return false end
    end
    return false
  end
end
