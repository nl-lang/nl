-- Operator definitions: `fn Vec.+(right)` and the others,
-- installed as metamethods.
--
-- The compiler names each definition by its event: "__add", "__sub",
-- "__unm" (unary `-`), "__bnot", "__len", "__index" (`[]`), "__newindex"
-- (`[]=`), "__call" (`()`), "__eq", "__lt", "__le", "__gt", "__ge",
-- "__coerce", "__iter" (`iter`), and the events of `//` and the bitwise
-- operators.
--
-- A struct's definitions are kept in I.info[S].ops; the dispatch of
-- nl_40_ops.lua takes the nearest one up the struct chain, which is how a
-- child struct inherits its parent's operators. It reads I.info[S].defs,
-- which holds that nearest definition for every event S or an ancestor
-- defines, so that applying an operator walks no chain: a definition is
-- entered in the `defs` of the struct and of every descendant that exists
-- when it runs (`define`), and a struct created later takes its parent's
-- (I.inherit_ops). What Lua sees is the generic metamethod of the event
-- (I.op_generic), set on the struct and on every descendant in the same
-- two places (rt.struct copies its parent's `__` keys first). `>` and
-- `>=` have no Lua event; only rt.gt and rt.ge read them.
--
-- `[]` makes the struct's `__index` a function: the struct's methods (and
-- its ancestors') first, then the definition, so it runs only for keys
-- that neither the instance nor the struct has and that are not declared
-- fields (likewise `[]=`, in nl_40_ops.lua). A struct that defines
-- `coerce` gets the generic metamethods of every event `coerce` covers,
-- so that `2 + x` reaches the dispatch when x's type defines no `+`.
--
-- Definitions on Number, String and Bool are kept in I.prim_defs and set
-- in the metatable of the type's values: always for strings; for numbers
-- and booleans only where the debug library can give them a metatable
-- (`nlc run` provides `debug.setmetatable` in its stand-in for the debug
-- library). Without it, a primitive's definition is reached only when the
-- right operand's type has a generic metamethod for the event, which
-- every struct defining the operator or `coerce` has.
--
-- Intrinsics: rt.opdef, rt.primop.
do
  local info_of = I.info
  local generic = I.op_generic

  -- The nearest definition of `ev` on S or its ancestors, found by
  -- walking the chain: what `defs[ev]` of S's description has to be.
  local function struct_def(S, ev)
    local guard = 0
    while S ~= nil and guard < 256 do
      local info = info_of[S]
      if info == nil or info.kind ~= "struct" then return nil end
      local d = info.ops and info.ops[ev]
      if d ~= nil then return d end
      S = info.parent
      guard = guard + 1
    end
    return nil
  end

  -- The `__index` of a struct with a `[]` definition (its own or
  -- inherited). `[]` runs only for keys that are neither methods nor
  -- declared fields: a declared field whose value is `nil`
  -- stays `nil`, as `@name` is always a plain field access.
  local function combined_index(S)
    local defs = info_of[S].defs
    return function(obj, k)
      local v = S[k]
      if v ~= nil then return v end
      if I.declared_field(obj, k) then return nil end
      local d = defs.__index
      if d == nil then return nil end
      return d(obj, k)
    end
  end

  -- Sets in D what Lua needs to reach the definition of `ev`.
  local function install(D, ev)
    if ev == "__index" then
      rawset(D, "__index", combined_index(D))
    elseif ev == "__coerce" then
      for _, e in ipairs(I.coerce_events) do
        if rawget(D, e) == nil then rawset(D, e, generic[e]) end
      end
    elseif generic[ev] ~= nil then
      rawset(D, ev, generic[ev])
    end
  end

  -- Defines `ev` on struct S and makes it reach S and its descendants.
  local function define(S, ev, f)
    local info = info_of[S]
    info.ops = info.ops or {}
    info.ops[ev] = f
    for D in pairs(I.structs) do
      local dinfo = info_of[D]
      if dinfo ~= nil and dinfo.ancestors and dinfo.ancestors[S] then
        dinfo.defs[ev] = struct_def(D, ev)
        install(D, ev)
      end
    end
  end
  I.define_op = define

  -- `fn S.<op>`: `ev` is the event of the operator, `f` the method.
  function rt.opdef(S, ev, f)
    local info = info_of[S]
    if info == nil or info.kind ~= "struct" then
      error("cannot define the operator " .. tostring(I.op_symbols[ev] or ev) .. " on "
        .. rt.typename(S) .. ": operators are defined on structs, Number, String and Bool", 2)
    end
    I.mark(f)
    define(S, ev, f)
  end

  -- A new child struct S: the definitions it inherits (each one is in its
  -- parent's `defs`), and the `__index` of an inherited `[]`. (rt.struct
  -- has copied the parent's other metamethods.)
  function I.inherit_ops(S)
    local defs = info_of[S].defs
    local pdefs = info_of[info_of[S].parent].defs
    for ev in pairs(pdefs) do defs[ev] = struct_def(S, ev) end
    if pdefs.__index ~= nil then install(S, "__index") end
  end

  -- The metatable of the values of a primitive type, created where
  -- possible.
  local function prim_metatable(luatype)
    if luatype == "string" then return getmetatable("") end
    if luatype == "number" then
      local mt = getmetatable(0)
      if mt == nil and debug_setmetatable then
        pcall(debug_setmetatable, 0, { __index = math })
        mt = getmetatable(0)
      end
      return mt
    end
    local mt = getmetatable(true)
    if mt == nil and I.setmetatable_bool then
      I.setmetatable_bool()
      mt = getmetatable(true)
    end
    return mt
  end

  local luatypes = { Number = "number", String = "string", Bool = "boolean" }
  local owners = {}

  -- `fn Number.<op>`, `fn String.<op>`, `fn Bool.<op>`.
  -- Like a primitive method, a definition is global to the Lua state, and
  -- redefining one that came from a different file is an error (a
  -- plain Exception), and the first definition stays. `file` is the
  -- owner of the defining chunk (rt.owner, nl_70_module.lua).
  function rt.primop(tname, ev, f, file)
    local lt = luatypes[tname]
    if lt == nil then error("not a primitive type: " .. tostring(tname), 2) end
    local defs = I.prim_defs[lt]
    local key = tname .. "." .. tostring(I.op_symbols[ev] or ev)
    if defs[ev] ~= nil and owners[key] ~= file then
      I.throw("Exception", "cannot redefine " .. key .. ": it is already defined by " .. tostring(owners[key]), 2)
    end
    owners[key] = file
    I.mark(f)
    defs[ev] = f
    local mt = prim_metatable(lt)
    if type(mt) == "table" and generic[ev] ~= nil then rawset(mt, ev, generic[ev]) end
  end
end
