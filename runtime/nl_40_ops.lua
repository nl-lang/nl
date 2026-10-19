-- Operators: the dispatch of operator definitions,
-- the operators that stock LuaJIT has no events for, and the comparisons;
-- ranges, iteration for `for ... in`, `case` value matching, conversions
-- and NL's `tostring`.
--
-- DISPATCH (on stock LuaJIT; the fork's events replace it).
-- Operator definitions are kept by event name ("__add", "__lt", "__gt",
-- "__coerce", ...): for a struct in I.info[S].ops, and with those it
-- inherits in I.info[S].defs (nl_42_opdef.lua), for Number, String and
-- Bool in I.prim_defs. `perform` applies NL's operator rules to one
-- binary operation:
--
--   1. two numbers (two strings for `<` and `<=`, strings and numbers for
--      `<>`): the built-in operation;
--   2. the definition of the left operand's type, the nearest one up the
--      struct chain (so a child struct inherits its parent's operators);
--   3. a metamethod written in Lua on the left operand;
--   4. when the left operand is a number, string or boolean: `coerce` of
--      the right operand's type, once; the operation is retried on the two
--      values it returns, without coerce;
--   5. a metamethod written in Lua on the right operand (Lua's rule, which
--      metamethods written in Lua keep);
--   6. otherwise an error. The right operand's NL definition is never used.
--
-- What Lua sees is one generic metamethod per event (I.op_generic), which
-- calls `perform` with the operands in their order (having taken steps 1
-- and 2 itself; see `perform`); nl_42_opdef.lua installs it on the types
-- that define operators. Generated code uses
-- Lua's arithmetic and concatenation operators directly (their events
-- reach the generic metamethods), `rt.idiv` and the bitwise intrinsics for
-- the events stock LuaJIT lacks (`rt.nfloor`, `rt.nband` and its
-- siblings, below, when the analyzer proves both operands numbers), and
-- `rt.lt`, `rt.le`,
-- `rt.gt`, `rt.ge` for ordering: LuaJIT compares values of different
-- types without asking a metamethod, and has no `>` or `>=` events.
-- `==` and `!=` between two
-- values that may be tables go through `rt.eq` (the left operand decides;
-- see there); comparisons with a literal number, string, boolean or nil
-- stay Lua's `==`.
--
-- Intrinsics: rt.idiv, rt.band, rt.bor, rt.bxor, rt.shl, rt.shr, rt.ushr,
-- rt.nfloor, rt.nband, rt.nbor, rt.nbxor, rt.nlshift, rt.narshift,
-- rt.nrshift, rt.nbnot, rt.bnot, rt.lt, rt.le, rt.gt, rt.ge, rt.eq, rt.range, rt.iter, rt.step,
-- rt.when, rt.as, rt.tostring, rt.unpack.
do
  local floor = math.floor
  local band, bor, bxor, lshift, arshift, rshift, bnot = bit.band, bit.bor, bit.bxor, bit.lshift, bit.arshift, bit.rshift, bit.bnot
  local info_of = I.info

  -- Floor division: never a negative zero. `floor(0 / -9)`
  -- is -0.0 in the interpreter, while the JIT of a dual-number LuaJIT
  -- (ARM64) narrows the same floor to the integer 0, so one program would
  -- print both; Lua 5.3's integer `//` gives 0, and so does this.
  local function idiv(a, b)
    local q = floor(a / b)
    if q == 0 then return 0 end
    return q
  end

  -- The NL spelling of each event, for messages and redefinition checks.
  local symbols = {
    __add = "+", __sub = "-", __mul = "*", __div = "/", __mod = "%", __pow = "**", __concat = "<>",
    __idiv = "//", __band = "&", __bor = "|", __bxor = "^", __shl = "<<", __shr = ">>", __ushr = ">>>",
    __eq = "==", __lt = "<", __le = "<=", __gt = ">", __ge = ">=",
    __unm = "-", __bnot = "~", __len = "len", __index = "[]", __newindex = "[]=", __call = "()",
    __coerce = "coerce", __tostring = "To<String>",
  }
  I.op_symbols = symbols

  -- The binary events `coerce` covers: arithmetic, bitwise,
  -- concatenation and ordering; not `==`.
  I.coerce_events = {
    "__add", "__sub", "__mul", "__div", "__mod", "__pow", "__concat", "__idiv",
    "__band", "__bor", "__bxor", "__shl", "__shr", "__ushr", "__lt", "__le",
  }

  local builtin = {
    __add = function(a, b) return a + b end,
    __sub = function(a, b) return a - b end,
    __mul = function(a, b) return a * b end,
    __div = function(a, b) return a / b end,
    __mod = function(a, b) return a % b end,
    __pow = function(a, b) return a ^ b end,
    __concat = function(a, b) return a .. b end,
    __idiv = idiv,
    __band = band, __bor = bor, __bxor = bxor, __shl = lshift, __shr = arshift, __ushr = rshift,
    __lt = function(a, b) return a < b end,
    __le = function(a, b) return a <= b end,
  }

  local function is_builtin(ev, ta, tb)
    if ta == "number" and tb == "number" then return true end
    if ev == "__concat" then
      return (ta == "string" or ta == "number") and (tb == "string" or tb == "number")
    end
    if ev == "__lt" or ev == "__le" then return ta == "string" and tb == "string" end
    return false
  end

  -- Definitions on Number, String and Bool, by Lua type.
  local prim_defs = { number = {}, string = {}, boolean = {} }
  I.prim_defs = prim_defs

  -- The NL definition of `ev` for the type of `v`: the nearest one up the
  -- struct chain, or the primitive type's. A struct's description has
  -- the answer for every event (`defs`, kept by nl_42_opdef.lua), so no
  -- loop runs here: the JIT recorder gives up on a loop that the function
  -- it is recording calls ("inner loop in root trace"), and this is on
  -- the path of every operator a struct defines.
  local function find_def(v, ev)
    local tv = type(v)
    local defs = prim_defs[tv]
    if defs then return defs[ev] end
    if tv ~= "table" then return nil end
    local S = getmetatable(v)
    if S == nil then return nil end
    local info = info_of[S]
    if info == nil or info.kind ~= "struct" then return nil end
    return info.defs[ev]
  end
  I.find_def = find_def

  local generic = {}
  local is_generic = setmetatable({}, { __mode = "k" })
  I.op_generic = generic
  -- The fork's VM calls a generic for the coerce rule when the left
  -- operand of an event is a number, string or boolean without a handler
  -- and the right operand's type has `coerce` but no handler for the
  -- event (LuaJIT/src/lj_meta.c, lj_nl_rightmm). The generics are never
  -- marked as methods: a marked handler on the right operand is an NL
  -- method, which the VM refuses to call for the left one.
  rt.opgeneric = generic

  -- A metamethod written in Lua (not one of the generic ones).
  local function lua_handler(v, ev)
    local mt = getmetatable(v)
    if type(mt) ~= "table" then return nil end
    local h = rawget(mt, ev)
    if h ~= nil and not is_generic[h] then return h end
    return nil
  end

  -- An operator that the left operand's type does not define (also
  -- after `coerce`) raises a plain Lua error, like the VM's own error for
  -- an operand without a metamethod; `try` catches it as a LuaError.
  -- Every path from generated code to here is a chain of tail calls, so
  -- level 2 is the NL code that applied the operator.
  local function fail(ev, a, b)
    local sym = symbols[ev] or ev
    if ev == "__lt" or ev == "__le" then
      error("attempt to compare " .. rt.typeof(a) .. " with " .. rt.typeof(b) .. " ('" .. sym .. "')", 2)
    end
    error("attempt to perform '" .. sym .. "' on " .. rt.typeof(a) .. " and " .. rt.typeof(b), 2)
  end

  local primitive = { number = true, string = true, boolean = true }

  local function call2(f, a, b, bool)
    if not bool then return f(a, b) end
    if f(a, b) then return true end
    return false
  end

  -- One binary operation under NL's operator rules (see the header).
  -- `bool`: the result is made a boolean (comparisons).
  --
  -- TAIL CALLS. A definition or a metamethod is called in a tail call
  -- wherever its result is returned as it is, so that an error it raises
  -- at level 2 names the NL line that applied the operator, as `fail`
  -- does. The JIT recorder counts every tail call of a trace toward its
  -- loop-unroll limit (15; see `numeric` below), so the path to an NL
  -- definition has as few as that allows: this function calls the
  -- definition itself (`call2` written in), and the generic metamethods
  -- below call it before they come here.
  local function perform(ev, a, b, coerce, bool)
    local ta, tb = type(a), type(b)
    if is_builtin(ev, ta, tb) then return builtin[ev](a, b) end
    local d = find_def(a, ev)
    if d ~= nil then
      if not bool then return d(a, b) end
      if d(a, b) then return true end
      return false
    end
    local h = lua_handler(a, ev)
    if h ~= nil then return call2(h, a, b, bool) end
    if coerce and primitive[ta] then
      -- A struct's `coerce`, or a `__coerce` written in a Lua metatable.
      local c = find_def(b, "__coerce") or lua_handler(b, "__coerce")
      if c ~= nil then
        local x, y = c(b, a)
        return perform(ev, x, y, false, bool)
      end
    end
    h = lua_handler(b, ev)
    if h ~= nil then return call2(h, a, b, bool) end
    return fail(ev, a, b)
  end
  I.perform = perform

  -- A unary operation: the operand's definition, or a metamethod written
  -- in Lua.
  local function perform_unary(ev, a)
    local d = find_def(a, ev)
    if d ~= nil then return d(a) end
    local h = lua_handler(a, ev)
    if h ~= nil then return h(a, a) end
    error("attempt to perform '" .. (symbols[ev] or ev) .. "' on " .. rt.typeof(a), 2)
  end
  I.perform_unary = perform_unary

  local function add_generic(ev, f)
    generic[ev] = f
    is_generic[f] = true
  end
  -- The first two steps of `perform` are taken here, so that the left
  -- operand's definition is one tail call away from the code that applied
  -- the operator and not three: a loop body with five operators of a
  -- struct was over the recorder's limit, and ran in the interpreter.
  for _, ev in ipairs(I.coerce_events) do
    add_generic(ev, function(a, b)
      if not is_builtin(ev, type(a), type(b)) then
        local d = find_def(a, ev)
        if d ~= nil then return d(a, b) end
      end
      return perform(ev, a, b, true)
    end)
  end
  local function add_unary(ev)
    add_generic(ev, function(a)
      local d = find_def(a, ev)
      if d ~= nil then return d(a) end
      return perform_unary(ev, a)
    end)
  end
  -- `>` and `>=`: the fork's CMPGT/CMPGE ask the left
  -- operand's `__gt`/`__ge` and fall back to the swapped `<`/`<=` without
  -- one, so a struct gets the generic only when it defines the operator
  -- (nl_42_opdef.lua installs by event). Lua code cannot reach them.
  add_generic("__gt", function(a, b) return perform("__gt", a, b, false, true) end)
  add_generic("__ge", function(a, b) return perform("__ge", a, b, false, true) end)
  add_unary("__unm")
  add_unary("__bnot")
  add_unary("__len")
  -- What Lua code comparing NL values reaches: Lua asks `__eq` only for
  -- two tables (or two userdata) with the same `__eq` metamethod, so Lua
  -- code keeps Lua's rule. NL code uses rt.eq.
  add_generic("__eq", function(a, b)
    local d = find_def(a, "__eq")
    if d == nil then return false end
    return d(a, b)
  end)
  -- `[]=` runs only for keys that are not declared fields:
  -- a declared field is always a plain field, as `@name` is, so an
  -- optional field can be assigned and `@name = v` inside `[]=` does not
  -- recurse. I.declared_field is in nl_20_struct.lua.
  add_generic("__newindex", function(obj, k, v)
    local d = find_def(obj, "__newindex")
    if d == nil or I.declared_field(obj, k) then return rawset(obj, k, v) end
    return d(obj, k, v)
  end)
  -- `fn Name.iter`: what `for v in x` iterates over.
  add_generic("__iter", function(obj)
    local d = find_def(obj, "__iter")
    if d == nil then error("attempt to iterate over a " .. rt.typeof(obj) .. " value", 2) end
    return d(obj)
  end)
  add_generic("__call", function(obj, ...)
    local d = find_def(obj, "__call")
    if d == nil then error("attempt to call a " .. rt.typeof(obj) .. " value", 2) end
    return d(obj, ...)
  end)
  -- `To<String>`: what `tostring` and interpolation produce.
  -- The result must be a string, as for `as String` (rt.as); a value that
  -- is not raises ConversionError.
  add_generic("__tostring", function(obj)
    local d = find_def(obj, "__tostring")
    if d == nil then return rt.typeof(obj) end
    local s = d(obj)
    if type(s) ~= "string" then
      I.throw("ConversionError", "cannot convert " .. rt.typeof(obj) .. " to String: To<String> returned "
        .. rt.typeof(s), 2)
    end
    return s
  end)

  -- The intrinsic of an operator that stock LuaJIT has no event for.
  -- The number path binds the result instead of returning `f(a, b)`: the
  -- JIT recorder counts every tail call toward its loop-unroll limit
  -- (lj_record_tailcall), so a loop body with more than about seven of
  -- these operators never got a trace and was blacklisted (bench/README.md,
  -- "Opcode decisions"). The other path stays a tail call (see `fail`).
  local function numeric(f, event)
    return function(a, b)
      if type(a) == "number" and type(b) == "number" then
        local r = f(a, b)
        return r
      end
      return perform(event, a, b, true)
    end
  end

  -- nl_arith: one operation under NL's rules, by event name.
  function rt.arith(ev, a, b)
    if ev == "__unm" or ev == "__bnot" then
      if type(a) == "number" then
        if ev == "__unm" then return -a end
        return bnot(a)
      end
      return perform_unary(ev, a)
    end
    return perform(ev, a, b, true)
  end

  -- `numeric(idiv, "__idiv")` with `idiv` written in: one Lua call less
  -- per `//`, which pays in the interpreter for the tail call `numeric`
  -- gave up.
  function rt.idiv(a, b)
    if type(a) == "number" and type(b) == "number" then
      local q = floor(a / b)
      if q == 0 then return 0 end
      return q
    end
    return perform("__idiv", a, b, true)
  end
  rt.band = numeric(band, "__band")
  rt.bor = numeric(bor, "__bor")
  rt.bxor = numeric(bxor, "__bxor")
  rt.shl = numeric(lshift, "__shl")
  rt.shr = numeric(arshift, "__shr")
  rt.ushr = numeric(rshift, "__ushr")

  -- What `//` and the bitwise operators call when the analyzer proves the
  -- operands numbers (crates/nl_ir/src/lower/operator.rs): the library
  -- functions as they were when the runtime opened, so that a script
  -- replacing `math.floor` or `bit.band` changes typed and untyped
  -- operators alike. Level-1 bytecode (the fork) emits the
  -- VM's bit opcodes instead of calling the bitwise ones, with the same
  -- results.
  rt.nfloor = floor
  rt.nband, rt.nbor, rt.nbxor, rt.nbnot = band, bor, bxor, bnot
  rt.nlshift, rt.narshift, rt.nrshift = lshift, arshift, rshift

  function rt.bnot(a)
    if type(a) == "number" then
      local r = bnot(a) -- not a tail call, as in `numeric`
      return r
    end
    return perform_unary("__bnot", a)
  end

  -- Ordering. `a > b` runs the `>` of a's type when it has
  -- one, and is otherwise `b < a`, where b's type decides; `>=` likewise
  -- with `<=`. Results are booleans, as Lua makes them.
  function rt.lt(a, b)
    local ta = type(a)
    if ta == type(b) and (ta == "number" or ta == "string") then return a < b end
    return perform("__lt", a, b, true, true)
  end

  function rt.le(a, b)
    local ta = type(a)
    if ta == type(b) and (ta == "number" or ta == "string") then return a <= b end
    return perform("__le", a, b, true, true)
  end

  function rt.gt(a, b)
    local ta = type(a)
    if ta == type(b) and (ta == "number" or ta == "string") then return a > b end
    local d = find_def(a, "__gt")
    if d ~= nil then return call2(d, a, b, true) end
    return perform("__lt", b, a, true, true)
  end

  function rt.ge(a, b)
    local ta = type(a)
    if ta == type(b) and (ta == "number" or ta == "string") then return a >= b end
    local d = find_def(a, "__ge")
    if d ~= nil then return call2(d, a, b, true) end
    return perform("__le", b, a, true, true)
  end

  -- `a == b` in NL code (`a != b` is `not rt.eq(a, b)`).
  -- `==` is consulted only between two tables or two userdata, and the
  -- left operand decides: when a's type has an NL `==` definition (its own
  -- or inherited), it runs, whatever b's type; values of different Lua
  -- types are unequal without a call. Otherwise Lua's `==`, so that a
  -- definition written in Lua (`__eq` set by Lua code) keeps Lua's rule.
  -- A value is equal to itself without a call, as in Lua.
  --
  -- The generic `__eq` above stays installed, so Lua code comparing the
  -- same values keeps Lua's rule: stock LuaJIT calls `__eq` only when both
  -- operands have the same handler, and the fork has no mechanism for the
  -- left-operand rule: it would need one (an `__eq` that is asked whenever
  -- the left operand has it), so the left-operand rule holds for NL code
  -- only.
  function rt.eq(a, b)
    if rawequal(a, b) then return true end
    local ta = type(a)
    if ta ~= type(b) then return false end
    if ta == "table" then
      local d = find_def(a, "__eq")
      if d ~= nil then
        if d(a, b) then return true end
        return false
      end
    end
    return a == b
  end

  -- Ranges: `a..b` includes `b`, `a...b` does not.
  local Range = {}
  Range.__index = Range
  I.Range = Range
  I.typeof_special[Range] = "Range"
  I.info[Range] = { kind = "builtin", name = "Range" }

  function rt.range(first, last, exclusive)
    return setmetatable({ first = first, last = last, exclusive = exclusive == true }, Range)
  end

  function rt.torange(v)
    if getmetatable(v) == Range then return v.first, v.last, v.exclusive end
  end

  function Range.__tostring(r)
    return tostring(r.first) .. (r.exclusive and "..." or "..") .. tostring(r.last)
  end

  -- `n` in the range.
  function I.range_contains(r, n)
    if type(n) ~= "number" then return false end
    if n < r.first then return false end
    if r.exclusive then return n < r.last end
    return n <= r.last
  end

  -- `step`: a number other than zero; a negative step counts
  -- down, as Lua's numeric `for` does. Zero would never end.
  function rt.step(s)
    if type(s) ~= "number" or s == 0 or s ~= s then
      error("a `step` must be a number other than zero, got " .. (type(s) == "number" and tostring(s) or rt.typeof(s)), 2)
    end
    return s
  end

  -- The iterators of a range, for Lua's generic `for`: the state is the
  -- last number, the control the number before the next one.
  --
  -- Neither they nor rt.iter make a closure. The JIT recorder compiles
  -- neither the creation of a closure (FNEW) nor the closing of upvalues
  -- that a function containing one does before every return (UCLO), so a
  -- `for` that got a closure from rt.iter kept every hot loop and
  -- function around it out of a trace.
  local function range_next(last, i)
    i = i + 1
    if i <= last then return i end
  end
  local function range_next_excl(last, i)
    i = i + 1
    if i < last then return i end
  end

  -- The array values of `t` in order, up to the first nil. The control
  -- is the index, and the value comes after it (rt.iter with `n` 0).
  local function array_next(t, i)
    i = i + 1
    local v = t[i]
    if v ~= nil then return i, v end
  end

  -- The same values from a closure, alone (rt.iter with `n` 1). In a
  -- function of its own, so that rt.iter contains no closure.
  local function array_iter(t)
    local i = 0
    return function()
      i = i + 1
      return t[i]
    end
  end

  -- `for` over a value: `n` is the number of loop variables.
  -- A function uses Lua's protocol; a range counts; a value whose
  -- metatable has `__iter` (`fn Name.iter`) uses what it
  -- returns, which is what Lua's generic `for` takes: an iterator function
  -- and optionally a state and a first control value; a string yields its
  -- characters; a table yields its array values for one variable and its
  -- pairs for more.
  --
  -- With `n` 0, the loop has one variable and keeps the iterator's
  -- control in a hidden variable of its own (crates/nl_ir,
  -- lower/control.rs): `for control, v in f, state, first`. A table and a
  -- string then need no closure to keep their place (array_next above,
  -- I.string_next in nl_80_string.lua): their iterators return the
  -- control and then the value. A function, a range and `__iter` return
  -- the value first, as in Lua, and it is the control. The loop must know
  -- which kind it got, so the first kind comes with a fourth result,
  -- rt.iter itself. A plain `true` would not do: what `__iter` returns is
  -- returned from here by a tail call (so that an error it raises at
  -- level 2 names the NL line), it may be more than three values, and
  -- this is one that no script can produce. `n` 1 is the form of
  -- compilers before the hidden variable: the value alone, from a
  -- closure for a table or a string.
  local function iter(n, x, s, c)
    local tx = type(x)
    if tx == "function" then return x, s, c end
    local mt = getmetatable(x)
    if mt == Range then
      if x.exclusive then return range_next_excl, x.last, x.first - 1 end
      return range_next, x.last, x.first - 1
    end
    if type(mt) == "table" and rawget(mt, "__iter") then return mt.__iter(x) end
    if tx == "string" then
      if n == 0 then return I.string_next, I.string_check(x, "chars"), 0, iter end
      return I.string_chars(x)
    end
    if tx == "table" then
      if n == 0 then return array_next, x, 0, iter end
      if n == 1 then return array_iter(x) end
      return next, x, nil
    end
    error("cannot iterate over a value of type " .. rt.typeof(x), 2)
  end
  rt.iter = iter

  -- A `when` item that is not a pattern: a Range matches by
  -- containment, a Regex by matching; anything else by `==`, with
  -- the subject on the left (rt.eq).
  function rt.when(subject, item)
    local mt = getmetatable(item)
    if mt == Range then return I.range_contains(item, subject) end
    if I.regex_when and mt ~= nil then
      local r = I.regex_when(item, subject)
      if r ~= nil then return r end
    end
    return rt.eq(subject, item)
  end

  -- NL's `tostring`: Lua's, which honours `__tostring`
  -- (where a `To<String>` implementation goes).
  rt.tostring = tostring

  rt.unpack = unpack

  -- Keyed arguments. `f a, k: v` passes the keyed
  -- arguments as one table after the positional ones, marked with the
  -- metatable KW so that a callee can tell it from a positional table. A
  -- `**opts` parameter takes it (rt.kwpop from the rest table, rt.iskw on
  -- each parameter, rt.kwtake to strip the mark); a function without one
  -- refuses it in a checked build (rt.nokw, rt.nokwrest). Lua code marks a
  -- table with the prelude function `keyed(t)`.
  local KW = { __name = "keyed arguments" }
  I.KW = KW
  -- Generated code tests the mark inline (`rt.getmetatable(p) == rt.KW`)
  -- and calls rt.nokw only when a parameter holds a marked table.
  rt.KW = KW
  local function iskw(v)
    return type(v) == "table" and getmetatable(v) == KW
  end
  rt.iskw = iskw

  -- `rt.keyed({k = v, …}, src, …)`: the written keys, then the string keys
  -- of each `**src` table that the written keys do not set. The written
  -- key wins whatever the order, so `f(**t, k: 1)` and `f(k: 1, **t)`
  -- agree. A new table each time: a `**` source is never changed.
  function rt.keyed(lit, ...)
    for i = 1, select("#", ...) do
      local src = select(i, ...)
      if src ~= nil then
        if type(src) ~= "table" then
          error("`**` needs a table of keyed arguments, got " .. rt.typeof(src), 2)
        end
        for k, v in pairs(src) do
          if type(k) == "string" and lit[k] == nil then lit[k] = v end
        end
      end
    end
    return setmetatable(lit, KW)
  end

  -- `keyed(t)` for Lua code: a copy of t's string keys, marked.
  function rt.keyed_copy(t)
    if type(t) ~= "table" then
      error("keyed() needs a table, got " .. rt.typeof(t), 2)
    end
    return rt.keyed({}, t)
  end

  -- The last item of the rest table when it is the keyed table, removed
  -- from the table; nil otherwise.
  function rt.kwpop(rest)
    local n = #rest
    local last = rest[n]
    if iskw(last) then
      rest[n] = nil
      return last
    end
    return nil
  end

  -- The last of the arguments beyond the parameters, when it is the keyed
  -- table; nil otherwise.
  function rt.kwlast(...)
    local n = select("#", ...)
    if n == 0 then return nil end
    local last = select(n, ...)
    if iskw(last) then return last end
    return nil
  end

  -- The `**opts` value: the marked table as a plain table, or an empty one.
  function rt.kwtake(kw)
    if kw == nil then return {} end
    return setmetatable(kw, nil)
  end

  -- A function without `**opts` that received keyed arguments in a
  -- parameter: a checked build raises.
  local function refuse(fname, kw)
    local keys = {}
    for k in pairs(kw) do keys[#keys + 1] = tostring(k) end
    table.sort(keys)
    I.throw("TypeError", fname .. " takes no keyed arguments, got " .. table.concat(keys, ", "), 3)
  end
  function rt.nokw(fname, ...)
    for i = 1, select("#", ...) do
      local v = select(i, ...)
      if iskw(v) then refuse(fname, v) end
    end
  end
  function rt.nokwrest(fname, rest)
    local last = rest[#rest]
    if iskw(last) then refuse(fname, last) end
  end

  -- `[a, *list, b]`: the items of each part, in order. A
  -- part is a table; its items are 1 to #part, so a nil inside a part
  -- is kept and a trailing nil is cut, as in a literal.
  function rt.listcat(...)
    local out, n = {}, 0
    for i = 1, select("#", ...) do
      local part = select(i, ...)
      if type(part) ~= "table" then
        error("`*` in a list needs a table, got " .. rt.typeof(part), 2)
      end
      local m = #part
      for j = 1, m do out[n + j] = part[j] end
      n = n + m
    end
    return out
  end

  -- `f(*list, k: v)`: every item of the list, then the keyed table.
  function rt.unpackkw(list, kw)
    local n = #list
    local t = { unpack(list, 1, n) }
    t[n + 1] = kw
    return unpack(t, 1, n + 1)
  end

  -- `x as T`, in order: already a T; `To<T>` of x's type,
  -- then `From<S>` of T (I.convert, nl_22_iface.lua); the built-in
  -- conversions; otherwise ConversionError. The result of a `To` or
  -- `From` conversion must be a T (P1): one that is not, nil included
  -- when nil is not a T, raises ConversionError too.
  function rt.as(v, t)
    if rt.isa(v, t) then return v end
    local ok, result, via = I.convert(v, t)
    if ok then
      if not rt.isa(result, t) then
        I.throw("ConversionError", "cannot convert " .. rt.typeof(v) .. " to " .. rt.typename(t)
          .. ": " .. via .. " returned " .. rt.typeof(result), 2)
      end
      return result
    end
    if t == rt.T.String then
      result = tostring(v)
    elseif t == rt.T.Number then
      result = tonumber(v)
    elseif t == rt.T.Bool then
      return not not v
    end
    if result == nil and not rt.isa(nil, t) then
      I.throw("ConversionError", "cannot convert " .. rt.typeof(v) .. " to " .. rt.typename(t), 2)
    end
    return result
  end
end
