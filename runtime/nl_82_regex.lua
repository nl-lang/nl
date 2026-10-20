-- NL's Regex, the built-in type `Regex`.
-- The engine is the Rust regex crate's, in libnlc (crates/nl_regex),
-- reached through the C glue of the fork (LuaJIT/src/lib_regex.c), which
-- the host table carries as `host.regex`. Without it (stock LuaJIT, the
-- `nlc run` oracle) `Regex` exists as a type but `Regex.new` raises.
--
-- A compiled pattern is a userdata of the fork, whose metatable is this
-- file's `Regex` table: the library (`Regex.new`), the methods, and the
-- type for `is`, `typeof` and `rt.gettype`. `__methods` is
-- set, so a function reached through a regex is called as a method
-- (runtime/nl_30_member.lua; ISMETH on the fork). `re.pattern` is a field.
-- Match tables, replacement expansion, the pattern cache and the
-- conversion of positions between bytes and characters are here; the glue
-- works in bytes.
--
-- Choices where the language is silent, marked "unspecified":
--   the cache (`Regex.new` of one pattern gives one object): bounded at
--       CACHE_MAX patterns and emptied when full;
--   `init` of find and match: as String.index's (negative from the end;
--       before the start or past the end plus one gives nil);
--   successive matches (scan, replace_all, split): as the Rust crate's
--       find_iter: after an empty match the search moves one character
--       on, and an empty match right where the previous match ended is
--       skipped;
--   `re.split(s)` is `s.split(re)`, with the Ruby rules of
--       String.split and its options (keep_empty, limit): trailing
--       empty fields are dropped without them, and an empty
--       match ends a field only when the field is not empty, so the empty
--       pattern splits into characters without a leading "";
--   tostring(re) is "Regex(pattern)".
--
-- Used by the core and the String library: I.Regex, I.isregex,
-- I.regex_when.
do
  local R = type(host) == "table" and type(host.regex) == "table" and host.regex or nil
  local Regex = R and R.metatable or {}
  rt.Regex = Regex
  I.Regex = Regex
  I.typeof_special[Regex] = "Regex"
  -- A built-in type, not a struct: `x is Regex` holds for
  -- compiled patterns, and rt.new cannot make one.
  I.info[Regex] = { kind = "builtin", name = "Regex" }
  Regex.__methods = true

  local bsub = lstring.sub
  local concat = table.concat
  local check, int_arg = I.string_check, I.int_arg
  local count, offset, advance, charpos = I.text.count, I.text.offset, I.text.advance, I.text.charpos

  local function isregex(v)
    return getmetatable(v) == Regex
  end
  I.isregex = isregex

  -- A Regex in `when`: matches a string subject; nil when
  -- `item` is not a Regex (rt.when, nl_40_ops.lua).
  function I.regex_when(item, subject)
    if not isregex(item) then return nil end
    if type(subject) ~= "string" then return false end
    return Regex.test(item, subject)
  end

  if R == nil then
    function Regex.new()
      I.throw("Exception", "Regex.new needs the regex library of the NL VM, which stock LuaJIT does not have", 2)
    end
    -- The methods below are reachable only through a compiled pattern,
    -- which cannot exist here; they are defined all the same, so that the
    -- library has one shape on both targets.
  end

  local compile = R and R.compile or function() return nil, "no regex library" end
  local names_of = R and R.names or function() return {} end
  local pattern_of = R and R.pattern or function() return "" end
  local rtest = R and R.test or function() return false end
  local rfind = R and R.find or function() return nil end
  local rcaptures = R and R.captures or function() return end

  -- `Regex.new(pattern)`, with the cache.
  local CACHE_MAX = 256
  local cache, ncache = {}, 0
  -- regex -> { [i] = name } for its named groups.
  local names = setmetatable({}, { __mode = "k" })

  if R ~= nil then
    function Regex.new(pattern)
      if type(pattern) ~= "string" then rt.typeerror("Regex.new argument 1", "String", pattern) end
      local re = cache[pattern]
      if re ~= nil then return re end
      local msg
      re, msg = compile(pattern)
      if re == nil then I.throw("RegexError", msg, 2) end
      if ncache >= CACHE_MAX then
        cache, ncache = {}, 0
      end
      cache[pattern] = re
      ncache = ncache + 1
      names[re] = names_of(re)
      return re
    end
  end

  function Regex.__index(re, k)
    if k == "pattern" then return pattern_of(re) end
    return rawget(Regex, k)
  end

  function Regex.__tostring(re)
    return "Regex(" .. pattern_of(re) .. ")"
  end

  local function receiver(re, fname)
    if not isregex(re) then rt.typeerror("Regex." .. fname .. " receiver", "Regex", re) end
    return re
  end

  -- The receiver and a checked subject.
  local function subject(re, s, fname)
    receiver(re, fname)
    return check(s, fname, 1, "Regex")
  end

  -- `init` (a character position, 1 unless given) as the byte to search
  -- from, or nil when nothing can match.
  local function init_byte(s, init, fname)
    init = int_arg(init, fname, 2, 1, "Regex")
    if init == 1 then return 1 end
    local n = count(s)
    if init < 0 then init = n + init + 1 end
    if init < 1 or init > n + 1 then return nil end
    return offset(s, init)
  end

  local function pack(...) return { n = select("#", ...), ... } end

  -- The match table of the spans `t` (a0, e0, a1, e1, ...
  -- as host.regex.captures gives them: 1-based bytes, the end exclusive).
  local function match_table(re, s, t)
    local m = {}
    local nm = names[re]
    for g = 0, t.n / 2 - 1 do
      local a = t[2 * g + 1]
      if a ~= nil then
        local v = bsub(s, a, t[2 * g + 2] - 1)
        m[g] = v
        local name = nm and nm[g]
        if name ~= nil then m[name] = v end
      end
    end
    return m
  end

  -- Successive matches of `re` in `s`: a function that returns the spans
  -- of the next match, or nil (see the header for the rules).
  local function matches(re, s)
    local pos, last_end, n = 1, nil, #s
    return function()
      while pos <= n + 1 do
        local t = pack(rcaptures(re, s, pos))
        if t.n == 0 then break end
        local a, e = t[1], t[2]
        if a == e and a == last_end then
          -- An empty match where the previous match ended: skipped.
          if a > n then break end
          pos = advance(s, a, 1)
        else
          last_end = e
          if e > a then
            pos = e
          elseif e > n then
            pos = n + 2
          else
            pos = advance(s, e, 1)
          end
          return t
        end
      end
      pos = n + 2
      return nil
    end
  end

  function Regex.test(re, s)
    subject(re, s, "test")
    return rtest(re, s)
  end

  -- `re.find(s, init = 1)`: the first and last character position of the
  -- first match, or nil. An empty match at position p gives p, p - 1, so
  -- that `s[first..last]` is the match.
  function Regex.find(re, s, init)
    subject(re, s, "find")
    local b = init_byte(s, init, "find")
    if b == nil then return nil end
    local a, e = rfind(re, s, b)
    if a == nil then return nil end
    return charpos(s, a), charpos(s, e) - 1
  end

  function Regex.match(re, s, init)
    subject(re, s, "match")
    local b = init_byte(s, init, "match")
    if b == nil then return nil end
    local t = pack(rcaptures(re, s, b))
    if t.n == 0 then return nil end
    return match_table(re, s, t)
  end

  -- `re.scan(s)`: an iterator over the match tables of all matches.
  function Regex.scan(re, s)
    subject(re, s, "scan")
    local nextm = matches(re, s)
    return function()
      local t = nextm()
      if t == nil then return nil end
      return match_table(re, s, t)
    end
  end

  local function replace(re, s, rep, all, fname)
    subject(re, s, fname)
    I.check_replacement(rep, fname, 2, "Regex")
    local out, k = {}, 0
    local i = 1
    local nextm = matches(re, s)
    while true do
      local t = nextm()
      if t == nil then break end
      local a, e = t[1], t[2]
      k = k + 1
      out[k] = bsub(s, i, a - 1)
      k = k + 1
      out[k] = I.replacement(rep, match_table(re, s, t), fname, "Regex")
      i = e
      if not all then break end
    end
    out[k + 1] = bsub(s, i)
    return concat(out)
  end

  function Regex.replace(re, s, rep)
    return replace(re, s, rep, false, "replace")
  end

  function Regex.replace_all(re, s, rep)
    return replace(re, s, rep, true, "replace_all")
  end

  -- `re.split(s)`: the pieces of `s` between the matches (see the header).
  -- `keep_empty` and `limit` as String.split's: every field kept (the
  -- empty string gives [""]), and at most `limit` fields.
  local split_opts = { keep_empty = true, limit = true }
  function Regex.split(re, s, opts)
    subject(re, s, "split")
    opts = I.string_opts(opts, "split", split_opts)
    local keep, limit = false, nil
    if opts ~= nil then
      keep = opts.keep_empty == true
      limit = I.string_limit(opts.limit, "split")
    end
    local out, k = {}, 0
    if s == "" then
      if keep then out[1] = "" end
      return out
    end
    local beg = 1
    local nextm = matches(re, s)
    while limit == nil or k < limit - 1 do
      local t = nextm()
      if t == nil then break end
      local a, e = t[1], t[2]
      if e > a or a > beg then
        k = k + 1
        out[k] = bsub(s, beg, a - 1)
        beg = e
      end
    end
    if beg <= #s or keep or limit ~= nil then
      k = k + 1
      out[k] = bsub(s, beg)
    end
    if not keep and limit == nil then
      while k > 0 and out[k] == "" do
        out[k] = nil
        k = k - 1
      end
    end
    return out
  end
end
