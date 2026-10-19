-- Structs, enums and modules. Interfaces
-- are in nl_22_iface.lua, operator definitions in nl_42_opdef.lua.
--
-- A struct is a table that is the metatable and the method table of its
-- instances (`S.__index = S`); a child's table inherits from its parent's
-- through its own metatable, and copies the parent's metamethods, since
-- Lua does not look metamethods up through `__index`. An instance is a
-- plain table. I.inherit_ops (nl_42_opdef.lua) completes what a child
-- inherits of its parent's operator definitions, and I.inherit_impls
-- (nl_22_iface.lua) gives it its ancestors' interfaces.
--
-- Intrinsics: rt.struct, rt.new, rt.newfield, rt.newdefault, rt.enum,
-- rt.module, rt.submodule.
do
  local info_of = I.info

  -- In a struct's metatable: the constructors of its literals by shape
  -- (see CONSTRUCTORS below).
  local SHAPES = {}

  -- Field descriptors: { name = "id", type = function() return T end,
  -- default = function() return value end } (see field_type below).
  function rt.struct(name, parent, fields, file)
    local S = {}
    S.__index = S
    local pinfo = parent ~= nil and info_of[parent] or nil
    if parent ~= nil and (not pinfo or pinfo.kind ~= "struct") then
      error("the parent of struct " .. name .. " is not a struct", 2)
    end
    local all, ancestors = {}, { [S] = true }
    if pinfo then
      for i = 1, #pinfo.fields do all[i] = pinfo.fields[i] end
      for a in pairs(pinfo.ancestors) do ancestors[a] = true end
      for k, v in pairs(parent) do
        if type(k) == "string" and ssub(k, 1, 2) == "__" and k ~= "__index" then S[k] = v end
      end
      setmetatable(S, { __index = parent })
    end
    for i = 1, #fields do all[#all + 1] = fields[i] end
    local fieldset = {}
    for i = 1, #all do fieldset[all[i].name] = true end
    -- Kept in S's metatable (nl_10_types.lua), so S stays collectable.
    -- `defs` and `ifaces` are S's own and inherited together: the nearest
    -- operator definition by event (nl_42_opdef.lua) and the set of
    -- implemented interfaces (nl_22_iface.lua), which the code that
    -- records a definition or an implementation keeps up to date. An
    -- operator and `x is I` then read one table, with no walk up the
    -- chain.
    info_of[S] = {
      kind = "struct", name = name, parent = parent, fields = all, fieldset = fieldset, ancestors = ancestors,
      file = file, defs = {}, ifaces = {},
    }
    rawset(getmetatable(S), SHAPES, {})
    if pinfo and I.inherit_ops then I.inherit_ops(S) end
    if pinfo and I.inherit_impls then I.inherit_impls(S) end
    return S
  end

  -- Whether `k` is a declared field (its own or inherited) of the struct
  -- instance `obj`: `[]` and `[]=` never run for one.
  function I.declared_field(obj, k)
    local info = info_of[getmetatable(obj)]
    return info ~= nil and info ~= false and info.fieldset ~= nil and info.fieldset[k] == true
  end

  -- A value is an instance of S when its raw metatable (a
  -- `__metatable` field neither hides an instance nor makes one) is S or
  -- a descendant, whatever the value's Lua type. The test generated code
  -- makes inline, `rt.getmetatable(v) == S` with the same function
  -- (crates/nl_ir, lower/types.rs), is the exact-type case of this one,
  -- so the two always agree.
  I.isa_kind.struct = function(v, t)
    local mt = getmetatable(v)
    local info = mt ~= nil and info_of[mt]
    return info ~= nil and info ~= false and info.kind == "struct" and info.ancestors[t] == true
  end

  -- The type of field `f`. Types are resolved at the first construction,
  -- when the struct can refer to itself, and kept; a type that names a
  -- struct declared later in the file is incomplete until that struct
  -- exists, so it is kept only once it is complete (I.PARTIAL), and a kept
  -- type that rejects a value is evaluated once more before the mismatch
  -- is reported. `f.inlined` is the kept type that a
  -- constructor tests inline (CONSTRUCTORS below): when the type
  -- evaluated again is another one, the constructors are made anew.
  local PARTIAL, OPTIONAL, UNION = I.PARTIAL, I.OPTIONAL, I.UNION
  local same_type, drop_constructors
  local function field_type(f, fresh)
    local t = not fresh and f.typeval or nil
    if t == nil then
      t = f.type()
      if t ~= nil and not (type(t) == "table" and rawget(t, PARTIAL)) then
        if f.inlined ~= nil and not same_type(f.inlined, t, 0) then drop_constructors() end
        f.typeval = t
      end
    end
    return t
  end

  -- The check of field `f` (of the struct described by `info`) against
  -- the value `v`.
  local function check_field(info, f, v)
    local t = field_type(f, false)
    if not rt.isa(v, t) and f.typeval ~= nil then t = field_type(f, true) end
    if not rt.isa(v, t) then
      I.throw("TypeError", "field '" .. f.name .. "' of " .. info.name .. " expects " .. rt.typename(t) .. ", got " .. rt.typeof(v))
    end
  end

  -- `Name { field: value }`: defaults for omitted fields, checks of the
  -- declared types.
  --
  -- The compiler builds the literal of a struct it can see whole (its
  -- fields, its parents' fields, their types and defaults) itself, as
  -- `rt.setmetatable({…}, S)` with the same steps in the same order
  -- (crates/nl_ir, lower/structs.rs): it fills a constant default in
  -- place and asks rt.newdefault for any other, tests a field type inline
  -- where it can (`rt.type(v) == "number"`) and calls rt.newfield when
  -- that test fails or when it has none. rt.new does the rest: a struct
  -- the compiler does not know (`Lib.Point`, a struct from an import, one
  -- reached through an alias), an unknown field, a `spawn do` block.
  --
  -- CONSTRUCTORS. rt.new is written with loops, over the fields and over
  -- the keys given, and the JIT does not compile a loop into the trace of
  -- the loop that calls it. So a literal's call names its keys (`shape`:
  -- the names, separated by commas). They are all the table can hold, so
  -- whether one of them is not a field is known from the struct and the
  -- shape. When none is, rt.new calls a constructor instead: a function
  -- generated once for the struct and the set of fields given, which does
  -- what the loops do for these fields, in their order, without a loop.
  -- For `Item { id: …, owner: … }`, with a default for `name` and no
  -- `note`:
  --
  --   local type, getmetatable, setmetatable, check, info, F, k1, k2 = ...
  --   return function(S, o)
  --     local v1 = o["id"]
  --     if not (type(v1) == "number") then check(info, F[1], v1) end
  --     local v2 = k1()
  --     if not (type(v2) == "string") then check(info, F[2], v2) end
  --     local v3 = o["owner"]
  --     if not ((v3 == nil or getmetatable(v3) == k2)) then check(info, F[3], v3) end
  --     local v4
  --     if not ((v4 == nil or type(v4) == "string")) then check(info, F[4], v4) end
  --     local obj = setmetatable({ ["id"] = v1, ["name"] = v2, ["owner"] = v3 }, S)
  --     return obj
  --   end
  --
  -- `check` is check_field and `F` the struct's fields; the constants are
  -- the default thunks and what the tests compare with. A test is the
  -- sufficient one that generated code makes (nl_10_types.lua, rt.type):
  -- when it fails, check_field applies the whole rule and raises. It is
  -- made from the field's kept type, so a struct gets its constructors
  -- once every typed field has one (the loops resolve them, in the
  -- constructions before), and field_type drops the constructors when a
  -- kept type changes. A type without such a test is checked by
  -- check_field alone.
  --
  -- The instance is a new table, as with the loops, with room for the
  -- fields that were given or have a default; in a trace the literal's
  -- own table is not built. Neither function ends in a tail call: LuaJIT
  -- counts those toward its unroll limit. The chunk reads no global, and
  -- has a name of the runtime's kind, so that its frames stay out of
  -- traces (nl_50_error.lua).
  --
  -- A constructor is made the second time a shape runs, long before
  -- LuaJIT finds a loop hot: a literal that runs once costs no
  -- compilation. A struct has no constructor, and its literals keep the
  -- loops, when it has more fields or constants than a Lua function
  -- takes, or a field whose name is not a string.
  --
  -- Without a shape the loops store every field, a nil one too, which
  -- leaves its key in the table. A constructor's table has the keys of
  -- the fields given or with a default, in declaration order, and so has
  -- the table the loops make for a shape that waits for its constructor:
  -- the fields read the same either way, and the instances of one
  -- literal are laid out alike (`for k, v in instance` visits their keys
  -- in one order).
  local MAX_FIELDS, MAX_CONSTANTS = 120, 40
  local lua_type = {}
  for name, lt in pairs({ Number = "number", String = "string", Bool = "boolean", Table = "table",
    Function = "function", Userdata = "userdata", Thread = "thread" }) do
    lua_type[rt.T[name]] = lt
  end

  -- The Lua expression that is true when the variable `v` holds a value
  -- that the complete type `t` accepts, or nil when the type has no
  -- inline test. `const(x)` names the constant `x`.
  local function inline_test(t, v, const, depth)
    if lua_type[t] then return "type(" .. v .. ') == "' .. lua_type[t] .. '"' end
    if t == rt.T.Nil then return v .. " == nil" end
    if t == rt.T.Any then return "true" end
    if type(t) ~= "table" or depth > 4 then return nil end
    local info = info_of[t]
    if info then
      -- I.isa_kind.struct for exactly `t`; I.isa_kind.enum.
      if info.kind == "struct" then return "getmetatable(" .. v .. ") == " .. const(t) end
      if info.kind == "enum" then return "(" .. v .. " ~= nil and " .. const(info.names) .. "[" .. v .. "] ~= nil)" end
      return nil
    end
    local inner = rawget(t, OPTIONAL)
    if inner ~= nil then
      local test = inline_test(inner, v, const, depth + 1)
      return test and ("(" .. v .. " == nil or " .. test .. ")")
    end
    local list = rawget(t, UNION)
    if list == nil or #list == 0 then return nil end
    local tests = {}
    for i = 1, #list do
      tests[i] = inline_test(list[i], v, const, depth + 1)
      if tests[i] == nil then return nil end
    end
    return "(" .. tconcat(tests, " or ") .. ")"
  end

  -- Whether the complete types `a` and `b` have the same inline test: one
  -- type, or optionals and unions of the same types, which are made anew
  -- each time a field type is evaluated.
  same_type = function(a, b, depth)
    if rawequal(a, b) then return true end
    if type(a) ~= "table" or type(b) ~= "table" or depth > 4 or info_of[a] or info_of[b] then return false end
    local x, y = rawget(a, OPTIONAL), rawget(b, OPTIONAL)
    if x ~= nil and y ~= nil then return same_type(x, y, depth + 1) end
    x, y = rawget(a, UNION), rawget(b, UNION)
    if x == nil or y == nil or #x ~= #y then return false end
    for i = 1, #x do
      if not same_type(x[i], y[i], depth + 1) then return false end
    end
    return true
  end

  -- A field is shared by a struct and its descendants, so every struct
  -- starts again: its shapes run with the loops once more.
  drop_constructors = function()
    for S in pairs(I.structs) do
      local info, m = info_of[S], getmetatable(S)
      if info and info.kind == "struct" then
        info.constructors = nil
        for i = 1, #info.fields do info.fields[i].inlined = nil end
      end
      if m ~= nil and rawget(m, SHAPES) then rawset(m, SHAPES, {}) end
    end
  end

  -- The constructor of the struct described by `info` for literals that
  -- give the fields in the set `given`; false when it has none, nil when
  -- a field type is not kept yet.
  local function constructor(info, given)
    local fields = info.fields
    local n = #fields
    if loadstring == nil or n > MAX_FIELDS then return false end
    for i = 1, n do
      local f = fields[i]
      if type(f.name) ~= "string" then return false end
      if f.type and f.typeval == nil then return nil end
    end
    local consts, names = {}, {}
    local function const(x)
      for i = 1, #consts do
        if rawequal(consts[i], x) then return names[i] end
      end
      consts[#consts + 1] = x
      names[#consts] = "k" .. #consts
      return names[#consts]
    end
    local code, items, inlined = {}, {}, {}
    for i = 1, n do
      local f = fields[i]
      local key, v = sformat("[%q]", f.name), "v" .. i
      local check = "check(info, F[" .. i .. "], " .. v .. ")"
      if given[f.name] then
        code[#code + 1] = "local " .. v .. " = o" .. key
        if f.default then
          code[#code + 1] = "if " .. v .. " == nil then " .. v .. " = " .. const(f.default) .. "() end"
        end
      elseif f.default then
        code[#code + 1] = "local " .. v .. " = " .. const(f.default) .. "()"
      else
        code[#code + 1] = "local " .. v
      end
      if f.type then
        local test = inline_test(f.typeval, v, const, 0)
        inlined[i] = test ~= nil and f.typeval or nil
        if test == nil then
          code[#code + 1] = check
        elseif test ~= "true" then
          code[#code + 1] = "if not (" .. test .. ") then " .. check .. " end"
        end
      end
      if given[f.name] or f.default then items[#items + 1] = key .. " = " .. v end
    end
    if #consts > MAX_CONSTANTS then return false end
    local source = "local type, getmetatable, setmetatable, check, info, F"
      .. (#names > 0 and ", " or "") .. tconcat(names, ", ") .. " = ...\nreturn function(S, o)\n"
      .. tconcat(code, "\n") .. "\nlocal obj = setmetatable({ " .. tconcat(items, ", ") .. " }, S)\nreturn obj\nend\n"
    local chunk = loadstring(source, "=[nl new " .. (smatch(tostring(info.name), "^[%w_.]+$") or "struct") .. "]")
    if chunk == nil then return false end
    for i = 1, n do
      if inlined[i] ~= nil then fields[i].inlined = inlined[i] end
    end
    return chunk(type, getmetatable, setmetatable, check_field, info, fields, unpack(consts, 1, #consts))
  end

  -- The constructor for the literals of `S` with the keys `shape`; false
  -- when rt.new's loops are to run: a key is not a field, S is not a
  -- struct, or it has no constructor (yet). `shapes[shape]` is false for
  -- good in the first case, otherwise the set of the fields given once
  -- the shape has run, then the constructor. Shapes with the same fields
  -- share one, kept in `info.constructors` by the numbers of those
  -- fields. While a shape that can have a constructor waits for it, the
  -- set is returned as well: the loops then store what the constructor
  -- will, so that the instances of one literal are all laid out alike.
  --
  -- The closure is there on purpose. A trace that gets here is recording
  -- one of a shape's first runs, and would keep their path, which ends in
  -- the interpreter, as the trace of the calling loop. LuaJIT does not
  -- record the creation of a closure (FNEW): it abandons the trace, and
  -- records the loop again later.
  local function shaped(S, shapes, shape)
    local info = info_of[S]
    if not info or info.kind ~= "struct" then return false end
    local fieldset, given, unknown = info.fieldset, shapes[shape], false
    local function take(name)
      if fieldset[name] then given[name] = true else unknown = true end
    end
    if given == nil then
      given = {}
      sgsub(shape, "[^,]+", take)
      if unknown or loadstring == nil or #info.fields > MAX_FIELDS then
        shapes[shape] = false
        return false
      end
      shapes[shape] = given
      return false, given
    end
    local which = {}
    for i = 1, #info.fields do
      if given[info.fields[i].name] then which[#which + 1] = i end
    end
    which = tconcat(which, ",")
    local made = info.constructors
    if made == nil then
      made = {}
      info.constructors = made
    end
    local make = made[which]
    if make == nil then
      make = constructor(info, given)
      made[which] = make
    end
    if make then shapes[shape] = make end
    return make or false, given
  end

  -- The description of the struct `S` being constructed. When S is not a
  -- struct, an error at `level` as the calling function counts it (2:
  -- that function's caller).
  local function struct_info(S, level)
    local info = info_of[S]
    if not info or info.kind ~= "struct" then
      local what = (type(S) == "table" and info) and tostring(info.name) or ("a " .. rt.typeof(S) .. " value")
      error("cannot construct " .. what .. ": it is not a struct", level + 1)
    end
    return info
  end

  -- `shape` comes with a literal, whose table is its own (see
  -- CONSTRUCTORS); the host (nl_new) and the runtime give none. `given`
  -- is set for a shape that waits for its constructor: the instance then
  -- gets the keys of the constructor's table first, in its order, and an
  -- omitted field without a default is not stored (it is nil).
  function rt.new(S, init, shape)
    local given
    if shape ~= nil then
      local m = getmetatable(S)
      local shapes = m ~= nil and rawget(m, SHAPES)
      if shapes then
        local make = shapes[shape]
        if make ~= false then
          if type(make) ~= "function" then make, given = shaped(S, shapes, shape) end
          if make then
            local obj = make(S, init)
            return obj
          end
        end
      end
    end
    local info = struct_info(S, 2)
    if type(init) ~= "table" then
      error("cannot construct " .. info.name .. ": the fields must be given as a table, not "
        .. rt.typeof(init), 2)
    end
    local obj, known = {}, {}
    if given ~= nil then
      for i = 1, #info.fields do
        local f = info.fields[i]
        if given[f.name] or f.default then obj[f.name] = true end
      end
    end
    for i = 1, #info.fields do
      local f = info.fields[i]
      known[f.name] = true
      local v = init[f.name]
      if v == nil and f.default then v = f.default() end
      if f.type then check_field(info, f, v) end
      if v ~= nil or given == nil or given[f.name] or f.default then obj[f.name] = v end
    end
    for k in pairs(init) do
      if not known[k] then
        error(info.name .. " has no field '" .. tostring(k) .. "'", 2)
      end
    end
    return setmetatable(obj, S)
  end

  -- The check rt.new makes of field `i` (counting the parents' fields
  -- first, as rt.struct lists them) of struct `S`, for a literal the
  -- compiler built: raises the TypeError rt.new would.
  function rt.newfield(S, i, v)
    local info = struct_info(S, 2)
    local f = info.fields[i]
    if f.type then check_field(info, f, v) end
  end

  -- The default of field `i` of struct `S`, evaluated as rt.new would.
  function rt.newdefault(S, i)
    return struct_info(S, 2).fields[i].default()
  end

  -- Enums: variants are fields; `values` lists the values in
  -- order; `name(v)` gives a variant's name.
  function rt.enum(name, variants)
    local E, values, names = {}, {}, {}
    for i = 1, #variants do
      local vname, value = variants[i][1], variants[i][2]
      E[vname] = value
      values[i] = value
      names[value] = vname
    end
    E.values = values
    E.name = function(v) return names[v] end
    info_of[E] = { kind = "enum", name = name, names = names }
    return E
  end

  I.isa_kind.enum = function(v, t, info)
    return v ~= nil and info.names[v] ~= nil
  end

  -- Modules.
  function rt.module(name)
    local M = {}
    info_of[M] = { kind = "module", name = name }
    return M
  end

  -- `module A.B.C`: the table at that path under `root`, created when
  -- missing.
  function rt.submodule(root, path, fullname)
    local t = root
    for i = 1, #path do
      local nxt = rawget(t, path[i])
      if nxt == nil then
        nxt = rt.module(fullname)
        t[path[i]] = nxt
      end
      t = nxt
    end
    return t
  end
end
