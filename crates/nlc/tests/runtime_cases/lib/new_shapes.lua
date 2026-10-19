-- rt.new with a shape, as a struct literal calls it, against rt.new without
-- one, which always runs its loops (runtime/nl_20_struct.lua, CONSTRUCTORS).
-- Two families of the same structs get the same constructions, one each
-- way: the results, the errors and the order of the thunks that ran must
-- be the same. And the instances of one literal are laid out alike,
-- whether the loops or a constructor made them: `pairs` visits their
-- keys in one order.
local rt = assert(NL_RT, "run with NLC_TEST_RT_GLOBAL=NL_RT")
local T = rt.T

local log, path = {}, "-"
local function note(s) log[#log + 1] = s end
-- A default thunk says who called it: rt.new's loops or a constructor.
local function default(name, value)
  return function()
    note("default " .. name)
    path = debug.getinfo(2, "S").source:find("nl new", 1, true) and "constructor" or "loops"
    return value
  end
end
local function typed(name, get)
  return function()
    note("type " .. name)
    return get()
  end
end

local function family()
  local F = {}
  F.Point = rt.struct("Point", nil, {
    { name = "x", type = typed("x", function() return T.Number end) },
    { name = "y", type = typed("y", function() return T.Number end), default = default("y", 0) },
  }, "new_shapes")
  F.Point3 = rt.struct("Point3", F.Point, {
    { name = "z", type = typed("z", function() return T.Number end), default = default("z", 0) },
  }, "new_shapes")
  F.Mode = rt.enum("Mode", { { "Easy", "Easy" }, { "Hard", "Hard" } })
  -- `Later` is a struct declared later: until then `next` is incomplete
  -- and `owner` is a shorter union.
  F.Thing = rt.struct("Thing", nil, {
    { name = "id", type = typed("id", function() return T.Number end) },
    { name = "label", type = typed("label", function() return T.String end), default = default("label", "none") },
    { name = "at", type = typed("at", function() return rt.optional(F.Point) end) },
    { name = "mode", type = typed("mode", function() return F.Mode end), default = default("mode", "Easy") },
    { name = "key", type = typed("key", function() return rt.union({ T.Number, T.String, T.Nil }) end) },
    { name = "next", type = typed("next", function() return rt.optional(F.Later) end) },
    { name = "owner", type = typed("owner", function() return rt.union({ F.Point, F.Later }) end) },
    { name = "rec", type = typed("rec", function() return rt.optional(rt.record({ a = T.Number })) end) },
    { name = "other", type = typed("other", function() return rt.negate(T.String) end) },
    { name = "free" },
    { name = "any", type = typed("any", function() return T.Any end), default = default("any", false) },
  }, "new_shapes")
  -- `Kind` is replaced by another struct on the way.
  F.Other = rt.struct("Other", nil, { { name = "x" } }, "new_shapes")
  F.Kind = F.Point
  F.Pair = rt.struct("Pair", nil, {
    { name = "c", default = default("c", 1) },
    { name = "a", type = typed("a", function() return F.Kind end) },
    { name = "b", type = typed("b", function() return rt.optional(F.Kind) end) },
  }, "new_shapes")
  -- Structs without a constructor: too many fields, a name that is not a
  -- string. `Quoted` has one.
  local many = {}
  for i = 1, 121 do
    many[i] = { name = "f" .. i, default = i % 40 == 0 and default("f" .. i, i) or nil }
  end
  F.Many = rt.struct("Many", nil, many, "new_shapes")
  F.Odd = rt.struct("Odd", nil, { { name = 1 }, { name = "v", default = default("v", "e") } }, "new_shapes")
  F.Quoted = rt.struct("Quoted", nil, { { name = "end", default = default("end", "e") }, { name = "a b\n\"c\"" } }, "new_shapes")
  F.Empty = rt.struct("Empty", nil, {}, "new_shapes")
  return F
end

local function show(v)
  if type(v) == "table" then return rt.typeof(v) end
  return tostring(v)
end

local function describe(ok, r, S)
  if not ok then
    return type(r) == "table" and (rt.typeof(r) .. ": " .. tostring(r.message)) or tostring(r)
  end
  local keys = {}
  for k in pairs(r) do keys[#keys + 1] = tostring(k) end
  local order = table.concat(keys, " ")
  table.sort(keys)
  for i, k in ipairs(keys) do keys[i] = k:gsub("%c", "?") .. "=" .. show(r[k] == nil and r[tonumber(k)] or r[k]) end
  return rt.typeof(r) .. (debug.getmetatable(r) == S and "" or " (another metatable)") .. " {" .. table.concat(keys, " ") .. "}", order
end

local A, B = family(), family()

-- One construction in each family: `init(F)` gives the fields.
local function construct(struct, init, shape)
  local SA, SB = A[struct] or struct, B[struct] or struct
  local ia, ib = init(A), init(B)
  log, path = {}, "-"
  local ok, r = pcall(rt.new, SA, ia)
  local a, alog = describe(ok, r, SA), table.concat(log, ", ")
  log, path = {}, "-"
  ok, r = pcall(rt.new, SB, ib, shape)
  local blog = table.concat(log, ", ")
  local b, order = describe(ok, r, SB)
  return b, blog, path, a == b and alog == blog, a, alog, order
end

-- `times` constructions, printed once for each run of equal results. A
-- shape gets its constructor the second time it runs.
local function step(label, struct, init, shape, times)
  local last, from, first
  local function flush(to)
    if last ~= nil then print(label .. (times and (" " .. from .. (to > from and ("-" .. to) or "")) or "") .. ": " .. last) end
  end
  for i = 1, times or 1 do
    local b, blog, bpath, same, a, alog, order = construct(struct, init, shape)
    local line = b .. " [" .. blog .. "] " .. bpath
    if not same then line = line .. "\n  NOT what the loops give: " .. a .. " [" .. alog .. "]" end
    first = first or order
    if order ~= first then line = line .. "\n  NOT the order of the first: " .. tostring(order) .. " after " .. first end
    if line ~= last then
      flush(i - 1)
      last, from = line, i
    end
  end
  flush(times or 1)
end

local function fields(t) return function() return t end end

step("point", "Point", fields({ x = 1 }), "x", 4)
step("point given", "Point", fields({ y = 2, x = 1 }), "y,x", 4)
step("point mistyped", "Point", fields({ x = "s" }), "x")
step("point missing", "Point", fields({}), "", 4)
step("point unknown", "Point", fields({ x = 1, y = 2, z = 3 }), "x,y,z", 4)
step("point twice", "Point", fields({ x = 4 }), "x,x", 4)
step("child", "Point3", fields({ z = 5, x = 1 }), "z,x", 4)
step("child nil", "Point3", fields({ x = 1 }), "x,y", 4)
step("child mistyped", "Point3", fields({ x = 1, y = true }), "x,y")

local function thing(extra)
  return function(F)
    local t = { id = 1, owner = rt.new(F.Point, { x = 0 }) }
    for k, v in pairs(extra and extra(F) or {}) do t[k] = v end
    return t
  end
end
step("thing early", "Thing", thing(), "id,owner", 4)
step("thing early next", "Thing", thing(function() return { next = 5 } end), "id,owner,next")
A.Later, B.Later = rt.struct("Later", nil, {}, "new_shapes"), rt.struct("Later", nil, {}, "new_shapes")
step("thing", "Thing", thing(), "id,owner", 3)
-- The union of `owner` grows when a `Later` is given.
step("thing later owner", "Thing", thing(function(F) return { owner = rt.new(F.Later, {}) } end), "id,owner", 4)
step("thing later next", "Thing", thing(function(F) return { next = rt.new(F.Later, {}) } end), "id,owner,next", 4)
step("thing full", "Thing", thing(function(F)
  return { label = "l", at = rt.new(F.Point3, { x = 1 }), mode = "Hard", key = "k", rec = { a = 1 }, other = 5, free = F }
end), "id,owner,label,at,mode,key,rec,other,free", 4)
for _, bad in ipairs({ { "id", "1" }, { "label", 5 }, { "at", 5 }, { "at", {} }, { "mode", "Medium" }, { "key", true },
  { "next", 5 }, { "owner", "o" }, { "rec", { a = "x" } }, { "other", "s" } }) do
  step("thing bad " .. bad[1] .. " " .. type(bad[2]), "Thing", thing(function() return { [bad[1]] = bad[2] } end), "id,owner," .. bad[1], 4)
end
step("thing unknown", "Thing", thing(function() return { nope = 1 } end), "id,owner,nope", 4)

local function pair(of)
  return function(F) return { a = rt.new(F[of], { x = 1 }), b = rt.new(F[of], { x = 2 }) } end
end
step("pair", "Pair", pair("Point"), "a,b", 4)
step("pair of a child", "Pair", pair("Point3"), "a,b", 2)
A.Kind, B.Kind = A.Other, B.Other
step("pair of the new kind", "Pair", pair("Other"), "a,b", 4)
step("pair of the old kind", "Pair", pair("Point"), "a,b", 2)

step("many", "Many", fields({ f1 = 1 }), "f1", 4)
step("odd", "Odd", fields({ "one" }), "", 4)
step("quoted", "Quoted", fields({ ["a b\n\"c\""] = 1 }), "a b\n\"c\"", 4)
step("empty", "Empty", fields({}), "", 4)
step("empty unknown", "Empty", fields({ x = 1 }), "x")

-- Not structs, with a shape.
step("a table", {}, fields({ x = 1 }), "x", 4)
step("false", false, fields({ x = 1 }), "x")
step("an enum", "Mode", fields({}), "")
local inst = rt.new(A.Point, { x = 1 })
step("an instance", inst, fields({ x = 1 }), "x", 4)

-- A table with a struct's metatable is described as the struct is; its
-- instances get the table as their metatable.
local ta, tb = setmetatable({}, debug.getmetatable(A.Point)), setmetatable({}, debug.getmetatable(B.Point))
local shared = 0
for i = 1, 4 do
  local a, b = rt.new(ta, { x = i }), rt.new(tb, { x = i }, "x")
  if debug.getmetatable(a) == ta and debug.getmetatable(b) == tb and b.x == i and b.y == 0 then shared = shared + 1 end
end
print("shared metatable: " .. shared)

-- A struct test reads the raw metatable.
A.Point.__metatable, B.Point.__metatable = "locked", "locked"
step("locked point", "Thing", thing(function(F) return { at = rt.new(F.Point, { x = 1 }) } end), "id,owner,at", 4)
step("spoof", "Thing", thing(function(F) return { at = setmetatable({}, { __metatable = F.Point }) } end), "id,owner,at", 4)

-- Random structs and literals, the same in both families: every kind of
-- field type with and without a default, parents, fields given in any
-- order or left out, values and defaults that are now and then nil or of
-- another type, and now and then a key that is no field. Each literal
-- runs three times.
local seed = 20261013
local function rnd(n)
  seed = seed * 16807 % 2147483647
  return seed % n + 1
end
local kinds = { "none", "number", "string", "bool", "any", "number?", "struct", "struct?", "number|string", "struct|nil",
  "enum", "record", "~string" }
local function type_thunk(F, kind, of)
  if kind == "number" then return function() return T.Number end end
  if kind == "string" then return function() return T.String end end
  if kind == "bool" then return function() return T.Bool end end
  if kind == "any" then return function() return T.Any end end
  if kind == "number?" then return function() return rt.optional(T.Number) end end
  if kind == "struct" then return function() return F[of] end end
  if kind == "struct?" then return function() return rt.optional(F[of]) end end
  if kind == "number|string" then return function() return rt.union({ T.Number, T.String }) end end
  if kind == "struct|nil" then return function() return rt.union({ F[of], T.Nil }) end end
  if kind == "enum" then return function() return F.Mode end end
  if kind == "record" then return function() return rt.record({ a = T.Number }) end end
  if kind == "~string" then return function() return rt.negate(T.String) end end
  return nil
end
-- Value number `n`, the same choice in both families: from 8 on, an
-- instance of one of the first `count` structs (a table when it cannot be
-- built from nothing).
local function value(F, n, count)
  if n == 1 then return 7 end
  if n == 2 then return "text" end
  if n == 3 then return true end
  if n == 4 then return "Hard" end
  if n == 5 then return { a = 1 } end
  if n == 6 then return { a = "one" } end
  if n == 7 or count == 0 then return nil end
  local ok, v = pcall(rt.new, F["R" .. ((n - 8) % count + 1)], {})
  return ok and v or {}
end

-- The number of a value for a field: mostly one that its type accepts.
local function pick(d, count)
  if rnd(5) == 1 then return rnd(7 + count) end
  local of = tonumber(d.of:sub(2))
  local instance = of <= count and 7 + of or 7
  local good = { number = { 1 }, string = { 2 }, bool = { 3 }, ["number?"] = { 1, 7 }, struct = { instance },
    ["struct?"] = { instance, 7 }, ["number|string"] = { 1, 2 }, ["struct|nil"] = { instance, 7 }, enum = { 4 },
    record = { 5 }, ["~string"] = { 1, 3, 7 } }
  local list = good[d.kind]
  return list and list[rnd(#list)] or rnd(7 + count)
end

local total, differ, constructed, raised, reordered = 0, 0, 0, 0, 0
for _ = 1, 40 do
  A, B = family(), family()
  local count, decls = 6, {}
  for s = 1, count do
    local decl = { parent = s > 1 and rnd(3) == 1 and ("R" .. rnd(s - 1)) or nil, fields = {} }
    for i = 1, rnd(6) - 1 do
      local d = { name = "r" .. s .. "f" .. i, kind = kinds[rnd(#kinds)], of = "R" .. rnd(s) }
      d.default = rnd(3) == 1 and pick(d, s - 1) or nil
      decl.fields[i] = d
    end
    decls["R" .. s] = decl
    for _, F in ipairs({ A, B }) do
      local fields = {}
      for i, d in ipairs(decl.fields) do
        fields[i] = { name = d.name, type = type_thunk(F, d.kind, d.of),
          default = d.default and default(d.name, value(F, d.default, s - 1)) or nil }
      end
      F["R" .. s] = rt.struct("R" .. s, decl.parent and F[decl.parent] or nil, fields, "new_shapes")
    end
  end
  local function fields_of(name)
    local decl = decls[name]
    local fields = decl.parent and fields_of(decl.parent) or {}
    for _, d in ipairs(decl.fields) do fields[#fields + 1] = d end
    return fields
  end
  for _ = 1, 60 do
    local name = "R" .. rnd(count)
    local keys, picks = {}, {}
    for _, d in ipairs(fields_of(name)) do
      if rnd(4) > 1 then
        table.insert(keys, rnd(#keys + 1), d.name)
        picks[d.name] = pick(d, count)
      end
    end
    if rnd(12) == 1 then
      keys[#keys + 1] = "zz"
      picks.zz = 1
    end
    local function init(F)
      local t = {}
      for _, key in ipairs(keys) do t[key] = value(F, picks[key], count) end
      return t
    end
    local first
    for _ = 1, 3 do
      local b, _, bpath, same, _, _, order = construct(name, init, table.concat(keys, ","))
      total = total + 1
      if not same then differ = differ + 1 end
      first = first or order
      if order ~= first then reordered = reordered + 1 end
      if bpath == "constructor" then constructed = constructed + 1 end
      if b:find("Error: ", 1, true) or b:find(" has no field ", 1, true) then raised = raised + 1 end
    end
  end
end
print("random: " .. total .. " constructions, " .. raised .. " raised, " .. constructed
  .. " with a default that a constructor evaluated, " .. differ .. " not what the loops give, "
  .. reordered .. " not in the order of the literal's first")
