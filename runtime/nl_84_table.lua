-- NL's table library, the global `Table`: Lua's `table` functions and a
-- collection library after Ruby's Array, Hash and Enumerable (Rust's names
-- where Ruby has none).
--
-- Lua's `insert`, `remove`, `concat`, `sort`, `maxn` and `getn` are copied
-- in when the chunk opens, and `table` is the `__index` of the library, so
-- that a function a host adds to `table` later (gmod's `table.Copy`) is
-- still `Table.Copy`. `sort` stays Lua's sort in place; `sorted` and
-- `sort_by` return new lists.
--
-- Methods. In NL code a value typed `[T]`, `{[K]: V}` or `Table` has the
-- functions that take a table first as methods: `xs.map(f)` is
-- `Table.map(xs, f)`, and `xs.first` auto-calls. The analyzer resolves
-- the call from the type (crates/nl_sema/src/analyzer/expr.rs), in every
-- mode, since a plain table has no metatable to find them through at
-- runtime: on a value of no known type `x.map(f)` is a member call, which
-- finds no `map`. In a typed table a key named like a function is read
-- as `t["first"]`. A record's members are its fields, and so are those
-- of the `**opts` of a function. `fn Table.name` adds a function here
-- (rt.primmethod, nl_30_member.lua), unmarked, as these are.
--
-- Callbacks get `(value, index)` for a list and `(key, value)` for a map.
-- A function that returns a list returns a new one and leaves its
-- arguments alone, unless it says "in place".
--
-- Lists are the values at 1 .. #t. Unspecified: a list with holes is
-- whatever `#` makes of it, as in Lua's table library (`compact` alone
-- reads up to `maxn`). Maps are every key `next` gives, in `next`'s
-- order, which is unspecified: `sorted_keys` and `each_sorted` give an
-- order (see `key_lt`). Unspecified: `has_key` and `get` read a key
-- with rawget, as the map functions read `next`: a table is its own
-- pairs, and an `__index` (a struct's methods, a default) is not one.
--
-- Errors name the function: an argument of the wrong type, and an element
-- the function cannot use: `sum` takes numbers (and `sum(t, f)` numbers
-- from f); `min`, `max`, `sorted` without `lt`, and `min_by`, `max_by`,
-- `sort_by` on their keys, compare with `<`, so the values must be all
-- numbers, all strings, or values with the same `__lt` (Durations, Times).
--
--   LISTS                        RESULT
--   map(t, f)                    [f(v, i)]; a nil result leaves a hole
--   filter(t, f), select         the values for which f(v, i) is truthy
--   reject(t, f)                 the others
--   reduce(t, init?, f), fold    f(acc, v, i) over t from init; without
--                                init, from t[1] (nil for an empty list)
--   each(t, f)                   calls f(v, i); returns t
--   each_with_index(t, f)        the same
--   find(t, f)                   the first v with f(v, i) truthy, or nil
--   find_index(t, f_or_value)    its index, or nil
--   index_of(t, v, from?)        the first index i >= from with t[i] == v
--   includes(t, v), include      whether some t[i] == v
--   any(t, f?), all, none, one   by f(v, i), or by the values' truth
--   count(t, f_or_value?)        #t, or how many match
--   partition(t, f)              two lists: the matching, the others
--   group_by(t, f)               {f(v, i): [v, ...]}
--   tally(t)                     {v: number of times}
--   sort_by(t, f)                sorted by f(v) with <, stable
--   sorted(t, lt?)               sorted with lt(a, b) (default <), stable
--   min(t, lt?), max(t, lt?)     the least, greatest value; nil if empty
--   min_by(t, f), max_by(t, f)   the value with the least, greatest f(v)
--   sum(t, f?)                   0 + every v, or every f(v, i)
--   uniq(t, by?)                 the first of each value (or of each by(v))
--   flatten(t, depth?)           nested lists spliced in (all levels)
--   flat_map(t, f)               map, then one level of flatten
--   slice(t, from, to?)          t[from..to]; negative from the end; a range
--   take(t, n), drop(t, n)       the first n, all but the first n
--   take_while(t, f), drop_while
--   first(t, n?), last(t, n?)    a value, or a list of n
--   reverse(t)                   a reversed copy
--   zip(t, ...)                  [[t[i], u[i], ...]] for each i in t
--   each_slice(t, n), chunks     lists of n (the last may be shorter)
--   each_cons(t, n)              every run of n consecutive values
--   join(t, sep?)                the values as strings (rt.tostring), joined
--   compact(t)                   the values up to maxn without the nils
--   push(t, ...), unshift        add at the end, at the start; return t
--   pop(t), shift(t)             remove and return the last, first value
--   append(a, b)                 a new list: a then b
--   extend(a, b)                 b added to a, in place; returns a
--   shuffle(t, rng?), sample     a shuffled copy, a random value; rng(n)
--                                gives an integer in 1..n (math.random)
--   MAPS
--   keys(t), values(t)           lists, in next's order
--   entries(t)                   [[k, v], ...]
--   sorted_keys(t, lt?)          the keys, sorted (key_lt by default)
--   each_sorted(t, f)            f(k, v) in sorted_keys order; returns t
--   each_kv(t, f)                f(k, v) for every pair; returns t
--   map_values(t, f)             {k: f(k, v)}
--   map_keys(t, f)               {f(k, v): v}
--   map_kv(t, f)                 {k2: v2} where k2, v2 = f(k, v)
--   filter_kv(t, f), reject_kv   the pairs for which f(k, v) is truthy, not
--   invert(t)                    {v: k}
--   to_set(t)                    {v: true} for the values of a list
--   merge(a, b, ...)             a new table: a's pairs, then b's, ...
--   merge_into(a, b, ...)        the same, into a; returns a
--   deep_merge(a, b)             merge, recursing where both are tables
--   copy(t), deep_copy(t)        a shallow, a deep copy
--   equal(a, b), deep_equal      the same keys and equal (==) values
--   is_empty(t), size(t)         no keys; how many keys
--   has_key(t, k)                rawget(t, k) ~= nil
--   get(t, k, default?)          rawget(t, k), or default when it is nil
--   dig(t, k1, k2, ...)          t[k1][k2]..., nil where a step is missing
--   pick(t, keys), omit(t, keys) a copy with only, without, those keys
--   clear(t)                     every key removed; returns t
--   CONSTRUCTORS
--   range(a, b, step?)           [a, a + step, ...] up to b
--   fill(n, v)                   n copies of v, or f(i) for a function
--   from_iter(f, s, c)           the first values of a generic for
--   pack(...)                    the arguments, with n = their count; the
--                                count is `t.n`, while `t.len` is #t,
--                                which a nil among them may cut short
--   unpack(t, i?, j?)            t[i], ..., t[j]; j is the count of a rest
--                                table, t.n when present, else #t
--
-- Used by the core: I.Table.
do
  local Table = {}
  rt.Table = Table
  I.Table = Table
  -- Where `fn Table.name` stores its function.
  I.prim_targets.Table = Table

  local tinsert = table.insert
  local random = math.random
  local maxn = table.maxn

  for _, name in ipairs({ "insert", "remove", "concat", "sort", "maxn", "getn" }) do
    Table[name] = table[name]
  end
  if Table.getn == nil then Table.getn = function(t) return #t end end
  setmetatable(Table, { __index = table })

  -- Arguments. An error names the library function, as String's do.
  local function tbl(t, fname, n)
    if type(t) ~= "table" then rt.typeerror("Table." .. fname .. " argument " .. (n or 1), "Table", t) end
    return t
  end

  local function callable(f)
    if type(f) == "function" then return true end
    local mt = getmetatable(f)
    return type(mt) == "table" and rawget(mt, "__call") ~= nil
  end

  local function fn(f, fname, n)
    if not callable(f) then rt.typeerror("Table." .. fname .. " argument " .. (n or 2), "Function", f) end
    return f
  end

  local function int(v, fname, n)
    if type(v) ~= "number" or v ~= floor(v) or v == huge or v == -huge then
      I.throw("TypeError", "Table." .. fname .. " argument " .. n .. " expects an integer, got "
        .. (type(v) == "number" and tostring(v) or rt.typeof(v)))
    end
    return v
  end

  -- A predicate from a function or a value (find_index, count).
  local function matcher(f)
    if callable(f) then return f end
    return function(v) return v == f end
  end

  ---------------------------------------------------------------------------
  -- Transforming lists

  function Table.map(t, f)
    tbl(t, "map")
    fn(f, "map")
    local out = {}
    for i = 1, #t do out[i] = f(t[i], i) end
    return out
  end

  local function keep(t, f, want, fname)
    tbl(t, fname)
    fn(f, fname)
    local out, k = {}, 0
    for i = 1, #t do
      local v = t[i]
      if (not not f(v, i)) == want then
        k = k + 1
        out[k] = v
      end
    end
    return out
  end

  function Table.filter(t, f) return keep(t, f, true, "filter") end
  Table.select = Table.filter
  function Table.reject(t, f) return keep(t, f, false, "reject") end

  -- `reduce(t, f)` starts from the first value; `reduce(t, init, f)` from
  -- `init`, which may be nil.
  function Table.reduce(t, init, f)
    tbl(t, "reduce")
    local start = 1
    if f == nil then
      f, init, start = init, t[1], 2
    end
    fn(f, "reduce", 3)
    local acc = init
    for i = start, #t do acc = f(acc, t[i], i) end
    return acc
  end
  Table.fold = Table.reduce

  function Table.each(t, f)
    tbl(t, "each")
    fn(f, "each")
    for i = 1, #t do f(t[i], i) end
    return t
  end
  Table.each_with_index = Table.each

  function Table.partition(t, f)
    tbl(t, "partition")
    fn(f, "partition")
    local yes, no, a, b = {}, {}, 0, 0
    for i = 1, #t do
      local v = t[i]
      if f(v, i) then
        a = a + 1
        yes[a] = v
      else
        b = b + 1
        no[b] = v
      end
    end
    return yes, no
  end

  function Table.group_by(t, f)
    tbl(t, "group_by")
    fn(f, "group_by")
    local out = {}
    for i = 1, #t do
      local v = t[i]
      local key = f(v, i)
      local g = out[key]
      if g == nil then
        out[key] = { v }
      else
        g[#g + 1] = v
      end
    end
    return out
  end

  function Table.tally(t)
    tbl(t, "tally")
    local out = {}
    for i = 1, #t do
      local v = t[i]
      out[v] = (out[v] or 0) + 1
    end
    return out
  end

  local function not_number(fname, what, i, v)
    I.throw("TypeError", "Table." .. fname .. " expects numbers, got " .. rt.typeof(v) .. " " .. what .. i)
  end

  function Table.sum(t, f)
    tbl(t, "sum")
    local s = 0
    if f == nil then
      for i = 1, #t do
        local v = t[i]
        if type(v) ~= "number" then not_number("sum", "at index ", i, v) end
        s = s + v
      end
    else
      fn(f, "sum")
      for i = 1, #t do
        local v = f(t[i], i)
        if type(v) ~= "number" then not_number("sum", "from the function for index ", i, v) end
        s = s + v
      end
    end
    return s
  end

  -- Unspecified: what is a nested list, for flatten: a table without a
  -- metatable that is a list (it has t[1], or no key at all). A map or a
  -- struct instance stays a value.
  local function is_list(v)
    return type(v) == "table" and getmetatable(v) == nil and (v[1] ~= nil or next(v) == nil)
  end

  local function flatten_into(out, k, t, depth)
    for i = 1, #t do
      local v = t[i]
      if depth ~= 0 and is_list(v) then
        k = flatten_into(out, k, v, depth - 1)
      else
        k = k + 1
        out[k] = v
      end
    end
    return k
  end

  function Table.flatten(t, depth)
    tbl(t, "flatten")
    if depth ~= nil then int(depth, "flatten", 2) else depth = -1 end
    local out = {}
    flatten_into(out, 0, t, depth)
    return out
  end

  function Table.flat_map(t, f)
    tbl(t, "flat_map")
    fn(f, "flat_map")
    local out, k = {}, 0
    for i = 1, #t do
      local v = f(t[i], i)
      if is_list(v) then
        for j = 1, #v do
          k = k + 1
          out[k] = v[j]
        end
      else
        k = k + 1
        out[k] = v
      end
    end
    return out
  end

  function Table.uniq(t, by)
    tbl(t, "uniq")
    if by ~= nil then fn(by, "uniq") end
    local seen, out, k = {}, {}, 0
    for i = 1, #t do
      local v = t[i]
      local key = v
      if by ~= nil then key = by(v, i) end
      -- A nil or NaN key cannot be a table key: such values are kept.
      if key == nil or key ~= key then
        k = k + 1
        out[k] = v
      elseif not seen[key] then
        seen[key] = true
        k = k + 1
        out[k] = v
      end
    end
    return out
  end

  function Table.reverse(t)
    tbl(t, "reverse")
    local out, n = {}, #t
    for i = 1, n do out[i] = t[n + 1 - i] end
    return out
  end

  function Table.zip(t, ...)
    tbl(t, "zip")
    local others = { ... }
    local m = select("#", ...)
    for j = 1, m do tbl(others[j], "zip", j + 1) end
    local out = {}
    for i = 1, #t do
      local row = { t[i] }
      for j = 1, m do row[j + 1] = others[j][i] end
      out[i] = row
    end
    return out
  end

  local function slices(t, n, step, fname)
    tbl(t, fname)
    int(n, fname, 2)
    if n < 1 then I.throw("Exception", "Table." .. fname .. ": the size must be at least 1, got " .. n) end
    local out, k, len = {}, 0, #t
    local last = step == n and len or len - n + 1
    for i = 1, last, step do
      local s = {}
      for j = 0, n - 1 do
        if i + j > len then break end
        s[j + 1] = t[i + j]
      end
      k = k + 1
      out[k] = s
    end
    return out
  end

  function Table.each_slice(t, n) return slices(t, n, n, "each_slice") end
  Table.chunks = Table.each_slice
  function Table.each_cons(t, n) return slices(t, n, 1, "each_cons") end

  function Table.join(t, sep)
    tbl(t, "join")
    local parts = {}
    for i = 1, #t do
      local v = t[i]
      parts[i] = type(v) == "string" and v or rt.tostring(v)
    end
    return tconcat(parts, sep or "")
  end

  function Table.compact(t)
    tbl(t, "compact")
    local out, k = {}, 0
    for i = 1, maxn(t) do
      local v = t[i]
      if v ~= nil then
        k = k + 1
        out[k] = v
      end
    end
    return out
  end

  function Table.push(t, ...)
    tbl(t, "push")
    local n = #t
    for j = 1, select("#", ...) do t[n + j] = (select(j, ...)) end
    return t
  end

  function Table.pop(t)
    tbl(t, "pop")
    local n = #t
    if n == 0 then return nil end
    local v = t[n]
    t[n] = nil
    return v
  end

  function Table.shift(t)
    tbl(t, "shift")
    if #t == 0 then return nil end
    return tremove(t, 1)
  end

  function Table.unshift(t, ...)
    tbl(t, "unshift")
    local m = select("#", ...)
    for j = m, 1, -1 do tinsert(t, 1, (select(j, ...))) end
    return t
  end

  function Table.append(a, b)
    tbl(a, "append")
    tbl(b, "append", 2)
    local out, n = {}, #a
    for i = 1, n do out[i] = a[i] end
    for i = 1, #b do out[n + i] = b[i] end
    return out
  end

  function Table.extend(a, b)
    tbl(a, "extend")
    tbl(b, "extend", 2)
    local n = #a
    for i = 1, #b do a[n + i] = b[i] end
    return a
  end

  -- Fisher-Yates on a copy. `rng(n)` is an integer in 1 .. n.
  function Table.shuffle(t, rng)
    tbl(t, "shuffle")
    if rng ~= nil then fn(rng, "shuffle") else rng = random end
    local out, n = {}, #t
    for i = 1, n do out[i] = t[i] end
    for i = n, 2, -1 do
      local j = rng(i)
      out[i], out[j] = out[j], out[i]
    end
    return out
  end

  function Table.sample(t, rng)
    tbl(t, "sample")
    if rng ~= nil then fn(rng, "sample") else rng = random end
    local n = #t
    if n == 0 then return nil end
    return t[rng(n)]
  end

  ---------------------------------------------------------------------------
  -- Searching lists

  function Table.find(t, f)
    tbl(t, "find")
    fn(f, "find")
    for i = 1, #t do
      local v = t[i]
      if f(v, i) then return v end
    end
    return nil
  end

  function Table.find_index(t, f)
    tbl(t, "find_index")
    f = matcher(f)
    for i = 1, #t do
      if f(t[i], i) then return i end
    end
    return nil
  end

  function Table.index_of(t, v, from)
    tbl(t, "index_of")
    if from ~= nil then int(from, "index_of", 3) else from = 1 end
    for i = from, #t do
      if t[i] == v then return i end
    end
    return nil
  end

  function Table.includes(t, v)
    tbl(t, "includes")
    for i = 1, #t do
      if t[i] == v then return true end
    end
    return false
  end
  Table.include = Table.includes

  -- How many values pass f (or are truthy), stopping once `limit` is
  -- passed.
  local function passing(t, f, limit, fname)
    tbl(t, fname)
    if f ~= nil then fn(f, fname) end
    local k = 0
    for i = 1, #t do
      local v = t[i]
      local ok
      if f == nil then ok = v else ok = f(v, i) end
      if ok then
        k = k + 1
        if k > limit then return k end
      end
    end
    return k
  end

  function Table.any(t, f) return passing(t, f, 0, "any") > 0 end
  function Table.none(t, f) return passing(t, f, 0, "none") == 0 end
  function Table.one(t, f) return passing(t, f, 1, "one") == 1 end

  function Table.all(t, f)
    tbl(t, "all")
    if f ~= nil then fn(f, "all") end
    for i = 1, #t do
      local v = t[i]
      local ok
      if f == nil then ok = v else ok = f(v, i) end
      if not ok then return false end
    end
    return true
  end

  function Table.count(t, f)
    tbl(t, "count")
    if f == nil then return #t end
    f = matcher(f)
    local k = 0
    for i = 1, #t do
      if f(t[i], i) then k = k + 1 end
    end
    return k
  end

  local function lt_default(a, b) return a < b end

  -- Whether `<` takes a and b: two numbers, two strings, or two values
  -- with the same `__lt`, as Lua 5.1 and LuaJIT have it.
  local function comparable(a, b)
    local ta = type(a)
    if ta ~= type(b) then return false end
    if ta == "number" or ta == "string" then return true end
    local ma, mb = getmetatable(a), getmetatable(b)
    if type(ma) ~= "table" or type(mb) ~= "table" then return false end
    local f = ma.__lt
    return f ~= nil and f == mb.__lt
  end

  -- `what` is "" for the values, "the key of " for keys.
  local function incomparable(fname, what, i, a, j, b)
    I.throw("TypeError", "Table." .. fname .. " cannot compare " .. what .. "element " .. i .. " (" .. rt.typeof(a)
      .. ") with " .. what .. "element " .. j .. " (" .. rt.typeof(b) .. ")")
  end

  -- Before `<` is used on list[1..n]: every value comparable with the
  -- first (and so with each other), numbers first since they are most.
  local function check_lt(list, n, fname, what)
    local first = list[1]
    if type(first) == "number" then
      for i = 2, n do
        if type(list[i]) ~= "number" then incomparable(fname, what, i, list[i], 1, first) end
      end
      return
    end
    for i = 2, n do
      if not comparable(first, list[i]) then incomparable(fname, what, i, list[i], 1, first) end
    end
  end

  local function extreme(t, lt, fname, greatest)
    tbl(t, fname)
    if lt ~= nil then
      fn(lt, fname)
    else
      check_lt(t, #t, fname, "")
      lt = lt_default
    end
    local best = t[1]
    for i = 2, #t do
      local v = t[i]
      if greatest then
        if lt(best, v) then best = v end
      elseif lt(v, best) then
        best = v
      end
    end
    return best
  end

  function Table.min(t, lt) return extreme(t, lt, "min", false) end
  function Table.max(t, lt) return extreme(t, lt, "max", true) end

  -- The first value whose key is least (greatest).
  local function extreme_by(t, f, fname, greatest)
    tbl(t, fname)
    fn(f, fname)
    local n = #t
    if n == 0 then return nil end
    local best, bk = t[1], f(t[1], 1)
    local k1 = bk
    local nums = type(k1) == "number"
    for i = 2, n do
      local v = t[i]
      local k = f(v, i)
      if nums then
        if type(k) ~= "number" then incomparable(fname, "the key of ", i, k, 1, k1) end
      elseif not comparable(k1, k) then
        incomparable(fname, "the key of ", i, k, 1, k1)
      end
      if (greatest and bk < k) or (not greatest and k < bk) then best, bk = v, k end
    end
    return best
  end

  function Table.min_by(t, f) return extreme_by(t, f, "min_by", false) end
  function Table.max_by(t, f) return extreme_by(t, f, "max_by", true) end

  ---------------------------------------------------------------------------
  -- Sorting. A merge sort, so that `sorted` and `sort_by` are stable and an
  -- inconsistent comparison cannot fail as Lua's sort does ("invalid order
  -- function"); runs of up to 8 are sorted by insertion first.

  -- Sorts a[1..n] by lt, with `tmp` as scratch space.
  local function msort(a, n, lt)
    local run = 8
    for lo = 1, n, run do
      local hi = lo + run - 1
      if hi > n then hi = n end
      for i = lo + 1, hi do
        local v = a[i]
        local j = i - 1
        while j >= lo and lt(v, a[j]) do
          a[j + 1] = a[j]
          j = j - 1
        end
        a[j + 1] = v
      end
    end
    local src, dst = a, {}
    local width = run
    while width < n do
      for lo = 1, n, 2 * width do
        local mid = lo + width - 1
        local hi = lo + 2 * width - 1
        if mid > n then mid = n end
        if hi > n then hi = n end
        local i, j, k = lo, mid + 1, lo
        while i <= mid and j <= hi do
          -- Equal values keep their order: the right one goes first only
          -- when it is strictly less.
          if lt(src[j], src[i]) then
            dst[k] = src[j]
            j = j + 1
          else
            dst[k] = src[i]
            i = i + 1
          end
          k = k + 1
        end
        for p = i, mid do
          dst[k] = src[p]
          k = k + 1
        end
        for p = j, hi do
          dst[k] = src[p]
          k = k + 1
        end
      end
      src, dst = dst, src
      width = width * 2
    end
    if src ~= a then
      for i = 1, n do a[i] = src[i] end
    end
    return a
  end

  function Table.sorted(t, lt)
    tbl(t, "sorted")
    local out, n = {}, #t
    for i = 1, n do out[i] = t[i] end
    if lt ~= nil then
      fn(lt, "sorted")
    else
      check_lt(out, n, "sorted", "")
      lt = lt_default
    end
    return msort(out, n, lt)
  end

  function Table.sort_by(t, f)
    tbl(t, "sort_by")
    fn(f, "sort_by")
    local n = #t
    local keys, idx = {}, {}
    for i = 1, n do
      keys[i] = f(t[i], i)
      idx[i] = i
    end
    check_lt(keys, n, "sort_by", "the key of ")
    msort(idx, n, function(a, b) return keys[a] < keys[b] end)
    local out = {}
    for i = 1, n do out[i] = t[idx[i]] end
    return out
  end

  -- Unspecified: the order of keys of different types, for sorted_keys
  -- and each_sorted: numbers, then strings, then booleans (false first),
  -- then anything else by its tostring; numbers and strings by <.
  local rank = { number = 1, string = 2, boolean = 3 }
  local function key_lt(a, b)
    local ta, tb = type(a), type(b)
    local ra, rb = rank[ta] or 4, rank[tb] or 4
    if ra ~= rb then return ra < rb end
    if ra == 1 or ra == 2 then return a < b end
    if ra == 3 then return not a and b end
    return tostring(a) < tostring(b)
  end

  function Table.sorted_keys(t, lt)
    tbl(t, "sorted_keys")
    if lt ~= nil then fn(lt, "sorted_keys") else lt = key_lt end
    local out, n = {}, 0
    for k in next, t do
      n = n + 1
      out[n] = k
    end
    return msort(out, n, lt)
  end

  function Table.each_sorted(t, f)
    fn(f, "each_sorted")
    local keys = Table.sorted_keys(t)
    for i = 1, #keys do
      local k = keys[i]
      f(k, t[k])
    end
    return t
  end

  ---------------------------------------------------------------------------
  -- Slicing lists

  -- Unspecified: as for strings (nl_80_string.lua), positions outside the
  -- list are clamped and an empty range gives an empty list; negative
  -- positions count from the end.
  function Table.slice(t, from, to)
    tbl(t, "slice")
    local n = #t
    local exclusive = false
    local a, b, x = rt.torange(from)
    if a ~= nil then
      from, to, exclusive = a, b, x
    else
      int(from, "slice", 2)
      if to == nil then to = n else int(to, "slice", 3) end
    end
    if from < 0 then from = n + from + 1 end
    if to < 0 then to = n + to + 1 end
    if exclusive then to = to - 1 end
    if from < 1 then from = 1 end
    if to > n then to = n end
    local out = {}
    for i = from, to do out[i - from + 1] = t[i] end
    return out
  end

  local function range_copy(t, from, to)
    local out = {}
    for i = from, to do out[i - from + 1] = t[i] end
    return out
  end

  local function count_arg(n, fname)
    int(n, fname, 2)
    if n < 0 then I.throw("Exception", "Table." .. fname .. ": the count must not be negative, got " .. n) end
    return n
  end

  function Table.take(t, n)
    tbl(t, "take")
    count_arg(n, "take")
    local len = #t
    return range_copy(t, 1, n < len and n or len)
  end

  function Table.drop(t, n)
    tbl(t, "drop")
    count_arg(n, "drop")
    return range_copy(t, n + 1, #t)
  end

  function Table.take_while(t, f)
    tbl(t, "take_while")
    fn(f, "take_while")
    local n = #t
    local i = 1
    while i <= n and f(t[i], i) do i = i + 1 end
    return range_copy(t, 1, i - 1)
  end

  function Table.drop_while(t, f)
    tbl(t, "drop_while")
    fn(f, "drop_while")
    local n = #t
    local i = 1
    while i <= n and f(t[i], i) do i = i + 1 end
    return range_copy(t, i, n)
  end

  function Table.first(t, n)
    tbl(t, "first")
    if n == nil then return t[1] end
    return Table.take(t, n)
  end

  function Table.last(t, n)
    tbl(t, "last")
    local len = #t
    if n == nil then return t[len] end
    count_arg(n, "last")
    local from = len - n + 1
    return range_copy(t, from > 1 and from or 1, len)
  end

  ---------------------------------------------------------------------------
  -- Maps

  function Table.keys(t)
    tbl(t, "keys")
    local out, n = {}, 0
    for k in next, t do
      n = n + 1
      out[n] = k
    end
    return out
  end

  function Table.values(t)
    tbl(t, "values")
    local out, n = {}, 0
    for _, v in next, t do
      n = n + 1
      out[n] = v
    end
    return out
  end

  function Table.entries(t)
    tbl(t, "entries")
    local out, n = {}, 0
    for k, v in next, t do
      n = n + 1
      out[n] = { k, v }
    end
    return out
  end

  function Table.each_kv(t, f)
    tbl(t, "each_kv")
    fn(f, "each_kv")
    for k, v in next, t do f(k, v) end
    return t
  end

  function Table.map_values(t, f)
    tbl(t, "map_values")
    fn(f, "map_values")
    local out = {}
    for k, v in next, t do out[k] = f(k, v) end
    return out
  end

  function Table.map_keys(t, f)
    tbl(t, "map_keys")
    fn(f, "map_keys")
    local out = {}
    for k, v in next, t do
      local k2 = f(k, v)
      if k2 ~= nil then out[k2] = v end
    end
    return out
  end

  function Table.map_kv(t, f)
    tbl(t, "map_kv")
    fn(f, "map_kv")
    local out = {}
    for k, v in next, t do
      local k2, v2 = f(k, v)
      if k2 ~= nil then out[k2] = v2 end
    end
    return out
  end

  local function keep_kv(t, f, want, fname)
    tbl(t, fname)
    fn(f, fname)
    local out = {}
    for k, v in next, t do
      if (not not f(k, v)) == want then out[k] = v end
    end
    return out
  end

  function Table.filter_kv(t, f) return keep_kv(t, f, true, "filter_kv") end
  function Table.reject_kv(t, f) return keep_kv(t, f, false, "reject_kv") end

  function Table.invert(t)
    tbl(t, "invert")
    local out = {}
    for k, v in next, t do out[v] = k end
    return out
  end

  function Table.to_set(t)
    tbl(t, "to_set")
    local out = {}
    for i = 1, #t do
      local v = t[i]
      if v ~= nil then out[v] = true end
    end
    return out
  end

  function Table.merge_into(a, ...)
    tbl(a, "merge_into")
    for j = 1, select("#", ...) do
      local b = tbl((select(j, ...)), "merge_into", j + 1)
      for k, v in next, b do a[k] = v end
    end
    return a
  end

  function Table.merge(a, ...)
    tbl(a, "merge")
    local out = {}
    for k, v in next, a do out[k] = v end
    for j = 1, select("#", ...) do
      local b = tbl((select(j, ...)), "merge", j + 1)
      for k, v in next, b do out[k] = v end
    end
    return out
  end

  -- Unspecified: deep_merge recurses where both values are tables without
  -- a metatable; it copies what it merges, and shares every other value.
  local function plain(v) return type(v) == "table" and getmetatable(v) == nil end
  local function deep_merge(a, b)
    local out = {}
    for k, v in next, a do out[k] = v end
    for k, v in next, b do
      local old = out[k]
      if plain(old) and plain(v) then out[k] = deep_merge(old, v) else out[k] = v end
    end
    return out
  end

  function Table.deep_merge(a, b)
    tbl(a, "deep_merge")
    tbl(b, "deep_merge", 2)
    return deep_merge(a, b)
  end

  function Table.copy(t)
    tbl(t, "copy")
    local out = {}
    for k, v in next, t do out[k] = v end
    local mt = getmetatable(t)
    if mt ~= nil then setmetatable(out, mt) end
    return out
  end

  -- Unspecified: deep_copy copies values that are tables, keeping each
  -- one's metatable (not copied) and the shape of shared and cyclic
  -- references; keys are kept as they are.
  local function deep_copy(t, seen)
    local done = seen[t]
    if done ~= nil then return done end
    local out = {}
    seen[t] = out
    for k, v in next, t do
      if type(v) == "table" then v = deep_copy(v, seen) end
      out[k] = v
    end
    local mt = getmetatable(t)
    if mt ~= nil then setmetatable(out, mt) end
    return out
  end

  function Table.deep_copy(t)
    tbl(t, "deep_copy")
    return deep_copy(t, {})
  end

  local function shallow_equal(a, b)
    for k, v in next, a do
      if b[k] ~= v then return false end
    end
    for k in next, b do
      if a[k] == nil then return false end
    end
    return true
  end

  function Table.equal(a, b)
    tbl(a, "equal")
    tbl(b, "equal", 2)
    return shallow_equal(a, b)
  end

  -- Unspecified: deep_equal compares tables by their pairs, whatever their
  -- metatables, and other values with ==; keys are matched by identity.
  -- A pair of tables already being compared counts as equal, so cycles end.
  local function deep_equal(a, b, seen)
    if a == b then return true end
    if type(a) ~= "table" or type(b) ~= "table" then return false end
    local s = seen[a]
    if s ~= nil and s[b] then return true end
    if s == nil then
      s = {}
      seen[a] = s
    end
    s[b] = true
    for k, v in next, a do
      if not deep_equal(v, b[k], seen) then return false end
    end
    for k in next, b do
      if a[k] == nil then return false end
    end
    return true
  end

  function Table.deep_equal(a, b)
    return deep_equal(a, b, {})
  end

  function Table.is_empty(t)
    tbl(t, "is_empty")
    return next(t) == nil
  end

  function Table.size(t)
    tbl(t, "size")
    local n = 0
    for _ in next, t do n = n + 1 end
    return n
  end

  function Table.has_key(t, k)
    tbl(t, "has_key")
    return k ~= nil and rawget(t, k) ~= nil
  end

  function Table.get(t, k, default)
    tbl(t, "get")
    local v = rawget(t, k)
    if v == nil then return default end
    return v
  end

  function Table.dig(t, ...)
    tbl(t, "dig")
    local v = t
    for j = 1, select("#", ...) do
      if type(v) ~= "table" then return nil end
      v = v[(select(j, ...))]
    end
    return v
  end

  function Table.pick(t, keys)
    tbl(t, "pick")
    tbl(keys, "pick", 2)
    local out = {}
    for i = 1, #keys do
      local k = keys[i]
      out[k] = t[k]
    end
    return out
  end

  function Table.omit(t, keys)
    tbl(t, "omit")
    tbl(keys, "omit", 2)
    local drop = {}
    for i = 1, #keys do drop[keys[i]] = true end
    local out = {}
    for k, v in next, t do
      if not drop[k] then out[k] = v end
    end
    return out
  end

  function Table.clear(t)
    tbl(t, "clear")
    local k = next(t)
    while k ~= nil do
      t[k] = nil
      k = next(t)
    end
    return t
  end

  ---------------------------------------------------------------------------
  -- Constructors

  -- `range(a, b, step = 1)`: like `for i = a, b, step`, as a list.
  function Table.range(a, b, step)
    if type(a) ~= "number" then rt.typeerror("Table.range argument 1", "Number", a) end
    if type(b) ~= "number" then rt.typeerror("Table.range argument 2", "Number", b) end
    if step == nil then
      step = 1
    elseif type(step) ~= "number" then
      rt.typeerror("Table.range argument 3", "Number", step)
    end
    if step == 0 then I.throw("Exception", "Table.range: the step must not be 0") end
    local out, k = {}, 0
    for v = a, b, step do
      k = k + 1
      out[k] = v
    end
    return out
  end

  function Table.fill(n, v)
    int(n, "fill", 1)
    local out = {}
    if callable(v) then
      for i = 1, n do out[i] = v(i) end
    else
      for i = 1, n do out[i] = v end
    end
    return out
  end

  -- The first values of `for v in f, s, c`: `Table.from_iter(pairs(t))`
  -- gives the keys, `Table.from_iter(s.scan(re))` the matches.
  function Table.from_iter(f, s, c)
    fn(f, "from_iter", 1)
    local out, k = {}, 0
    while true do
      c = f(s, c)
      if c == nil then break end
      k = k + 1
      out[k] = c
    end
    return out
  end

  function Table.pack(...)
    return { n = select("#", ...), ... }
  end

  function Table.unpack(t, i, j)
    tbl(t, "unpack")
    if j == nil then j = I.count(t) end
    return unpack(t, i or 1, j)
  end
end
