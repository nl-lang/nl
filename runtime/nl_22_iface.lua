-- Interfaces and the conversions `To<T>` and `From<T>`.
--
-- An interface is a table described in I.info:
--   { kind = "interface", name = "Shape", methods = { "area", ... },
--     fields = { "name", ... } }
-- Type parameters are erased: `Container<T>` is one table.
--
-- Conformance is nominal. `interface I for T` (rt.implement) checks the
-- implementation against the declaration, records it in I.impls[T] and
-- makes the methods ordinary methods of T; when a check fails, nothing is
-- recorded or installed. T is a struct, or one of the
-- descriptors rt.T.Number, rt.T.String, rt.T.Bool for the primitive types,
-- whose methods go through rt.primmethod. A struct satisfies
-- the interfaces of its ancestors, as it has their methods: its
-- description has the set of them all (I.info[S].ifaces),
-- which `x is I` reads.
--
--   I.impls[T] = { interfaces = { [I] = true },
--                  to = { { type = U, fn = f }, ... },    -- each To<U>
--                  from = { { type = S, fn = f }, ... } } -- each From<S>
--
-- `To<T>` and `From<T>` are the exception to erasure:
-- their type argument is a runtime value, so a type implements them for
-- any number of type arguments, each recorded by its argument, as Rust
-- allows `impl From<A> for X` and `impl From<B> for X` side by side.
-- `x as T` picks `To<T>` by T and `From<S>` by the type of x; the methods
-- `to` and `from` are dispatchers that do the same,
-- and `x.to` raises when more than one `To` applies, as Rust's `x.into()`
-- needs a type annotation then.
--
-- Intrinsics: rt.interface, rt.implement, rt.To, rt.From.
do
  local info_of = I.info
  -- The records: a struct's in its description (I.info[S].impls), so that
  -- it does not keep the struct alive (its methods may refer to the
  -- struct; see nl_10_types.lua); the primitive types' here. `impls[T]`
  -- is the record of either; where T is known to be a primitive type,
  -- prim_impls is read directly.
  local prim_impls = {}
  local impls = setmetatable({}, {
    __index = function(_, T)
      local p = prim_impls[T]
      if p ~= nil then return p end
      local info = info_of[T]
      return info and info.impls
    end,
    __newindex = function(_, T, rec)
      local info = info_of[T]
      if info ~= nil and info.kind == "struct" then
        info.impls = rec
      else
        prim_impls[T] = rec
      end
    end,
  })
  I.impls = impls
  local dispatchers -- the methods `to` and `from`; defined below

  -- `interface Name ... end`: the names of its methods and required
  -- fields.
  function rt.interface(name, methods, fields, file)
    local IF = {}
    info_of[IF] = { kind = "interface", name = name, methods = methods, fields = fields, file = file }
    return IF
  end

  -- `To<T>` and `From<T>` are prelude interfaces.
  rt.To = rt.interface("To", { "to" }, {}, "=nl runtime")
  rt.From = rt.interface("From", { "from" }, {}, "=nl runtime")

  local prim_of = { number = rt.T.Number, string = rt.T.String, boolean = rt.T.Bool }
  local prim_name = { [rt.T.Number] = "Number", [rt.T.String] = "String", [rt.T.Bool] = "Bool" }

  -- The record of T's implementations, for each of the types `v` is an
  -- instance of, nearest first: `f(record, T, a)` until it returns
  -- non-nil. `f` is a function made once, and `a` what a closure would
  -- have captured: the JIT recorder does not compile the creation of a
  -- closure, so one made per call keeps every loop that converts a value
  -- out of a trace.
  local function each_record(v, f, a)
    local tv = type(v)
    local p = prim_of[tv]
    if p then
      local rec = prim_impls[p]
      if rec then
        local r = f(rec, p, a) -- not a tail call (see find_to)
        return r
      end
      return nil
    end
    if tv ~= "table" then return nil end
    local S = getmetatable(v)
    if S == nil then return nil end
    -- At most 256 types, as a counted loop that is left before its end
    -- when the answer is on v's own type or v's type has no parent: the
    -- recorder gives up on a loop inside the function it records only
    -- when the loop comes round ("inner loop in root trace"), so a
    -- conversion a struct defines itself can be compiled. One it inherits
    -- still walks the chain.
    for _ = 1, 256 do
      local info = info_of[S]
      if info == nil or info.kind ~= "struct" then return nil end
      local rec = impls[S]
      if rec then
        local r = f(rec, S, a)
        if r ~= nil then return r end
      end
      S = info.parent
      if S == nil then return nil end
    end
    return nil
  end

  -- Whether struct S or one of its ancestors implements IF, found by
  -- walking the chain: what `ifaces[IF]` of S's description has to be.
  local function struct_implements(S, IF)
    local guard = 0
    while S ~= nil and guard < 256 do
      local info = info_of[S]
      if info == nil or info.kind ~= "struct" then return false end
      local rec = impls[S]
      if rec and rec.interfaces[IF] then return true end
      S = info.parent
      guard = guard + 1
    end
    return false
  end

  -- A new child struct S: the interfaces it has through its ancestors
  -- (each one is in its parent's `ifaces`).
  function I.inherit_impls(S)
    local info = info_of[S]
    for IF in pairs(info_of[info.parent].ifaces) do
      if struct_implements(S, IF) then info.ifaces[IF] = true end
    end
  end

  -- `x is I`. A number, string or boolean is an `I`
  -- when `I` is implemented for its type. A struct's description has the
  -- set of the interfaces it and its ancestors implement (`ifaces`, kept
  -- by rt.implement and I.inherit_impls), so the test is a lookup: a
  -- walk up the chain here would keep every function
  -- with a parameter of an interface type out of a trace.
  I.isa_kind.interface = function(v, t)
    local tv = type(v)
    local p = prim_of[tv]
    if p then
      local rec = prim_impls[p]
      return rec ~= nil and rec.interfaces[t] == true
    end
    if tv ~= "table" then return false end
    local S = getmetatable(v)
    if S == nil then return false end
    local info = info_of[S]
    if info == nil or info.kind ~= "struct" then return false end
    return info.ifaces[t] == true
  end

  local function has_field(info, name)
    for i = 1, #info.fields do
      if info.fields[i].name == name then return true end
    end
    return false
  end

  -- `interface IF for T`: `methods` maps each method name to its function;
  -- `targ` is the type argument of `To<U>` or `From<S>` (nil for other
  -- interfaces, whose type arguments are erased).
  function rt.implement(IF, T, methods, targ, file)
    local iinfo = info_of[IF]
    if iinfo == nil or iinfo.kind ~= "interface" then
      error(rt.typename(IF) .. " is not an interface, so it cannot be implemented", 2)
    end
    local pname = prim_name[T]
    local tinfo = info_of[T]
    if pname == nil and (tinfo == nil or tinfo.kind ~= "struct") then
      error("interface " .. iinfo.name .. " cannot be implemented for " .. rt.typename(T)
        .. ": interfaces are implemented for structs, Number, String and Bool", 2)
    end
    local tname = pname or tinfo.name
    local slot = (IF == rt.To and "to") or (IF == rt.From and "from") or nil
    -- `To<U>` and `From<S>` are named with their type argument.
    local iname = iinfo.name
    if slot ~= nil and targ ~= nil then iname = iname .. "<" .. rt.typename(targ) .. ">" end
    local what = "interface " .. iname .. " for " .. tname
    -- Everything is checked before anything changes: an implementation
    -- that fails changes nothing (a rejected write leaves the state as
    -- it was).
    -- Every method of the declaration, and nothing else.
    local declared = {}
    for i = 1, #iinfo.methods do
      local m = iinfo.methods[i]
      declared[m] = true
      if methods[m] == nil then error(what .. " does not implement the method '" .. m .. "'", 2) end
    end
    for m in pairs(methods) do
      if not declared[m] then error(what .. ": '" .. tostring(m) .. "' is not a method of " .. iinfo.name, 2) end
    end
    -- Required fields: the struct (or one of its ancestors) declares them.
    for i = 1, #iinfo.fields do
      local f = iinfo.fields[i]
      if pname ~= nil or not has_field(tinfo, f) then
        error(what .. ": " .. tname .. " has no field '" .. f .. "', which " .. iinfo.name .. " requires", 2)
      end
    end
    local rec = impls[T]
    if slot ~= nil then
      if targ == nil then error(what .. ": the type argument must be a type checked at runtime", 2) end
      -- `To<String>` for a primitive type would change how every number,
      -- string or boolean of the state prints; not
      -- supported.
      if slot == "to" and pname ~= nil and targ == rt.T.String then
        error(what .. ": implementing To<String> for a primitive type is not supported", 2)
      end
    end
    -- A method of a primitive type that another file defined cannot be
    -- replaced: refused before anything is installed. The
    -- dispatchers `to` and `from` belong to the runtime and are shared
    -- by every file's conversions.
    if pname ~= nil then
      for name in pairs(methods) do
        local refusal = name ~= slot and I.primmethod_refusal(pname, name, file)
        if refusal then I.throw("Exception", refusal, 2) end
      end
    end
    if rec == nil then
      rec = { interfaces = {} }
      impls[T] = rec
    end
    if slot ~= nil then
      -- Recorded by type argument; implementing the same `To<U>` again
      -- replaces the earlier one, as a reopened method does.
      local list = rec[slot]
      if list == nil then
        list = {}
        rec[slot] = list
      end
      local entry = { type = targ, fn = methods[slot] }
      local replaced = false
      for i = 1, #list do
        if list[i].type == targ then
          list[i] = entry
          replaced = true
        end
      end
      if not replaced then list[#list + 1] = entry end
    end
    rec.interfaces[IF] = true
    -- T and the descendants that exist now have the interface; a struct
    -- created later takes it from its parent (I.inherit_impls).
    if pname == nil then
      for D in pairs(I.structs) do
        local dinfo = info_of[D]
        if dinfo ~= nil and dinfo.ancestors and dinfo.ancestors[T] and struct_implements(D, IF) then
          dinfo.ifaces[IF] = true
        end
      end
    end
    for name, f in pairs(methods) do
      local owner = file
      if name == slot then
        -- The dispatchers are owned by nobody (the standard library), so
        -- every file may install them and no file may redefine them.
        f = dispatchers[slot]
        owner = nil
      end
      if pname ~= nil then
        rt.primmethod(pname, name, f, owner)
      else
        I.mark(f)
        rawset(T, name, f)
      end
    end
    -- To<String> defines what `tostring` and interpolation give.
    if slot == "to" and targ == rt.T.String then I.define_op(T, "__tostring", methods.to) end
  end

  -- The value a `From` method receives as `self`: the type as NL code
  -- names it (`Celsius.from(x)`, `Number.from(x)`), and the type back
  -- from that value.
  -- (rt.String and I.Bool are defined by later files, so looked up late.)
  local function type_self(T)
    if T == rt.T.Number then return math end
    if T == rt.T.String then return rt.String end
    if T == rt.T.Bool then return I.Bool end
    return T
  end
  local function self_type(v)
    if v == math then return rt.T.Number end
    if v == rt.String then return rt.T.String end
    if v == I.Bool then return rt.T.Bool end
    return v
  end

  local function entry_for(list, t)
    for i = 1, #list do
      if list[i].type == t then return list[i] end
    end
    return nil
  end

  -- The `To<t>` of v: on its type, then its ancestors (a child
  -- struct uses its parent's). The results below are bound before
  -- they are returned: a tail call counts toward the JIT recorder's
  -- loop-unroll limit (see `numeric` in nl_40_ops.lua), and nothing here
  -- raises an error that counts frames.
  local function to_entry(rec, _, t)
    if rec.to ~= nil then
      local e = entry_for(rec.to, t)
      return e
    end
  end
  local function find_to(v, t)
    local e = each_record(v, to_entry, t)
    return e
  end

  -- The `From<S>` of t that takes v, and the reason when none or several
  -- do. The S that is v's own type wins, then the nearest ancestor of
  -- v's type; among the others (interfaces, unions) exactly one must
  -- accept v, as Rust refuses overlapping `From` implementations.
  local function from_entry(_, S, list)
    local e = entry_for(list, S)
    return e
  end
  local function find_from(t, v)
    local rec = impls[t]
    local list = rec and rec.from
    if list == nil then return nil end
    local found = each_record(v, from_entry, list)
    if found ~= nil then return found end
    local candidates = {}
    for i = 1, #list do
      if rt.isa(v, list[i].type) then candidates[#candidates + 1] = list[i] end
    end
    if #candidates == 1 then return candidates[1] end
    if #candidates == 0 then return nil end
    local names = {}
    for i = 1, #candidates do names[i] = "From<" .. rt.typename(candidates[i].type) .. ">" end
    return nil, "ambiguous: " .. rt.typename(t) .. " implements " .. table.concat(names, " and ")
      .. ", and " .. rt.typeof(v) .. " is each of them"
  end

  local function to_list(rec)
    if rec.to ~= nil and #rec.to > 0 then return rec.to end
  end

  -- The methods `to` and `from` that an implementation installs.
  dispatchers = {
    -- `x.to`: the sole `To` of x's type (nearest type with one); more than
    -- one is an error, as `x.into()` without an annotation is in Rust.
    to = function(self)
      local list = each_record(self, to_list)
      if list == nil then
        I.throw("ConversionError", rt.typeof(self) .. " implements no To", 2)
      end
      if #list > 1 then
        local names = {}
        for i = 1, #list do names[i] = "To<" .. rt.typename(list[i].type) .. ">" end
        I.throw("ConversionError", rt.typeof(self) .. " implements " .. table.concat(names, " and ")
          .. ": write `as " .. rt.typename(list[1].type) .. "` to say which", 2)
      end
      return (list[1].fn(self))
    end,
    -- `T.from(v)`: the `From<S>` of T for v's type.
    from = function(self, v)
      local t = self_type(self)
      local entry, why = find_from(t, v)
      if entry == nil then
        I.throw("ConversionError", "cannot convert " .. rt.typeof(v) .. " to " .. rt.typename(t)
          .. (why and (": " .. why) or ": no From takes a " .. rt.typeof(v)), 2)
      end
      return (entry.fn(self, v))
    end,
  }

  -- Steps 2 and 3 of `x as T`: true, the result and the
  -- name of the conversion (for messages) when a conversion applies,
  -- false otherwise. rt.as checks the result.
  function I.convert(v, t)
    local to = find_to(v, t)
    if to ~= nil then return true, (to.fn(v)), "To<" .. rt.typename(t) .. ">" end
    local from, why = find_from(t, v)
    if from ~= nil then
      return true, (from.fn(type_self(t), v)), "From<" .. rt.typename(from.type) .. ">"
    end
    if why ~= nil then
      I.throw("ConversionError", "cannot convert " .. rt.typeof(v) .. " to " .. rt.typename(t) .. ": " .. why, 2)
    end
    return false
  end
end
