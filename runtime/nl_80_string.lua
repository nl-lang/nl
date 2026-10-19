-- NL's string library, the global `String`. Positions are
-- character positions; the text is read as UTF-8.
--
-- Two sets of primitives. On the fork, the
-- host table carries `host.text`, the C glue (LuaJIT/src/lib_nltext.c)
-- over the text library of libnlc (crates/nl_text): validity remembered in
-- the string object, counting and position scans with an ASCII fast path,
-- substrings by character range, Unicode case mapping. Without it (stock
-- LuaJIT, the `nlc run` oracle) the Lua stand-ins below do the same work
-- in Lua, except that case mapping is ASCII-only (string_case_unicode.nl
-- is excluded there). Everything built on the primitives (search, split,
-- strip, padding, replacement) is the same Lua on both.
--
-- Encoding. The character operations check the whole string first and
-- raise EncodingError for invalid UTF-8 (overlong forms, surrogates, code
-- points above U+10FFFF, truncated or stray bytes). The byte-level ones
-- never do: `len` and `<>` are not here; `as_bytes`, `byteslice`,
-- `valid_encoding`, `format` and `rep` below do not check. A string
-- argument that is searched for or inserted by character (a pattern, a
-- separator, a prefix, a padding, a replacement, and the string a
-- replacement function returns) is checked too. Once a string is known
-- to be valid, a byte search for a valid needle can only stop at a
-- character boundary, so the searches below are byte searches.
--
-- Regex. A `Regex` argument to `sub`, `gsub`, `split`, `match`, `scan` is
-- handed to the Regex library (nl_82_regex.lua, I.Regex); a string pattern
-- is taken literally. `index`, `start_with` and `end_with` take strings.
--
-- Choices where the language is silent are in one place each, marked
-- "unspecified":
--   whitespace (strip, lstrip, rstrip, split without a separator):
--       ASCII space, \t, \n, \v, \f, \r (WS below);
--   s[a..b], byteslice: positions outside the string are clamped, as in
--       Lua's string.sub; an empty range gives "";
--   center, ljust, rjust: Ruby's (see `justify`);
--   split: Ruby's (see String.split);
--   $0 in a replacement: the whole match (see I.expand_replacement);
--   index's `init`, byteslice with one number, "".ord, n.chr outside
--       Unicode: at each function, marked "Unspecified".
--
-- Used by the core and by the Regex library: I.chars, I.string_chars,
-- I.string_next, I.strindex, I.string_check, I.int_arg, I.text (the
-- primitives count, offset, advance, charpos), I.replacement,
-- I.expand_replacement.
do
  local String = {}
  rt.String = String
  I.String = String

  local bsub, byte, find, char, gmatch, gsub = lstring.sub, lstring.byte, lstring.find, lstring.char, lstring.gmatch, lstring.gsub
  local upper, lower, srep = lstring.upper, lstring.lower, lstring.rep
  local concat = table.concat
  local floor = math.floor
  local huge = math.huge

  -- The fork's text library, or nil.
  local text = type(host) == "table" and type(host.text) == "table" and host.text or nil

  -- Unspecified: the whitespace that strip and friends remove, and that
  -- separates fields in `split` without a separator. Ruby's set without
  -- NUL; the same as Lua's `%s` on LuaJIT.
  local WS = " \t\n\v\f\r"
  local ws_byte = {}
  for i = 1, #WS do ws_byte[byte(WS, i)] = true end

  local charpattern = "[%z\1-\127\194-\244][\128-\191]*"

  ---------------------------------------------------------------------------
  -- Primitives. Each takes a valid UTF-8 string unless it says otherwise.
  --
  --   invalid_at(s)      1-based byte of the first invalid sequence, or nil
  --                      (any bytes)
  --   count(s)           the number of characters
  --   offset(s, p)       the byte where character p (1 .. count + 1) starts
  --   advance(s, i, k)   from byte i, the start of a character, k characters
  --                      forward; #s + 1 at the end
  --   charpos(s, b)      the character position of byte b, the start of a
  --                      character
  --   decode(s, i)       the code point starting at byte i, and the byte
  --                      after it
  --   encode(cp)         the UTF-8 string of a Unicode scalar value
  --   chars(s)           the characters, as a list
  --   codepoints(s)      the code points, as a list
  --   bytes(s)           the byte values, as a list (any bytes)
  --   at(s, i)           s[i] for a number i (negative from the end), or nil
  --   slice(s, a, b, x)  s[a..b] (s[a...b] when x), clamped, "" when empty
  --   upcase, downcase, capitalize, swapcase

  local invalid_at, count, offset, advance, charpos, decode, encode
  local chars, codepoints, bytes, at, slice
  local upcase, downcase, capitalize, swapcase

  -- Unspecified: a range is clamped to the string, as Lua's string.sub
  -- does; a range that selects nothing gives "". `n` is the length; `first`
  -- and `last` may be negative; an exclusive range stops before `last`.
  -- Returns 1-based inclusive bounds, or nil.
  local function clamp(first, last, n, exclusive)
    if first < 0 then first = n + first + 1 end
    if last < 0 then last = n + last + 1 end
    if exclusive then last = last - 1 end
    if first < 1 then first = 1 end
    if last > n then last = n end
    if first > last then return nil end
    return first, last
  end

  if text then
    invalid_at, count, offset, advance, charpos = text.invalid_at, text.count, text.offset, text.advance, text.charpos
    decode, encode = text.decode, text.encode
    chars, codepoints, bytes, at, slice = text.chars, text.codepoints, text.bytes, text.at, text.slice
    upcase, downcase, capitalize, swapcase = text.upcase, text.downcase, text.capitalize, text.swapcase
  else
    -- The byte position of the first invalid UTF-8 sequence, or nil.
    function invalid_at(s)
      local i = find(s, "[\128-\255]")
      local n = #s
      while i ~= nil do
        local c = byte(s, i)
        local len, lo, hi
        if c >= 0xC2 and c <= 0xDF then len, lo, hi = 2, 0x80, 0xBF
        elseif c == 0xE0 then len, lo, hi = 3, 0xA0, 0xBF      -- no overlong
        elseif c == 0xED then len, lo, hi = 3, 0x80, 0x9F      -- no surrogates
        elseif c >= 0xE1 and c <= 0xEF then len, lo, hi = 3, 0x80, 0xBF
        elseif c == 0xF0 then len, lo, hi = 4, 0x90, 0xBF      -- no overlong
        elseif c >= 0xF1 and c <= 0xF3 then len, lo, hi = 4, 0x80, 0xBF
        elseif c == 0xF4 then len, lo, hi = 4, 0x80, 0x8F      -- up to U+10FFFF
        else return i end                                      -- C0, C1, F5..FF, stray continuation
        if i + len - 1 > n then return i end
        local c2 = byte(s, i + 1)
        if c2 < lo or c2 > hi then return i end
        for k = i + 2, i + len - 1 do
          local ck = byte(s, k)
          if ck < 0x80 or ck > 0xBF then return i end
        end
        i = find(s, "[\128-\255]", i + len)
      end
      return nil
    end

    -- The number of characters: the bytes that are not continuation bytes.
    function count(s)
      local _, n = gsub(s, "[^\128-\191]", "")
      return n
    end

    function advance(s, i, k)
      local n = #s
      for _ = 1, k do
        if i > n then return n + 1 end
        i = find(s, "[^\128-\191]", i + 1) or (n + 1)
      end
      return i
    end

    function offset(s, p)
      return advance(s, 1, p - 1)
    end

    function charpos(s, b)
      if b <= 1 then return 1 end
      return count(bsub(s, 1, b - 1)) + 1
    end

    function decode(s, i)
      local c = byte(s, i)
      if c == nil then return nil end
      if c < 0x80 then return c, i + 1 end
      if c < 0xE0 then return (c - 0xC0) * 0x40 + (byte(s, i + 1) - 0x80), i + 2 end
      if c < 0xF0 then
        local c2, c3 = byte(s, i + 1, i + 2)
        return ((c - 0xE0) * 0x40 + (c2 - 0x80)) * 0x40 + (c3 - 0x80), i + 3
      end
      local c2, c3, c4 = byte(s, i + 1, i + 3)
      return (((c - 0xF0) * 0x40 + (c2 - 0x80)) * 0x40 + (c3 - 0x80)) * 0x40 + (c4 - 0x80), i + 4
    end

    function encode(cp)
      if cp < 0x80 then return char(cp) end
      if cp < 0x800 then return char(0xC0 + floor(cp / 0x40), 0x80 + cp % 0x40) end
      if cp < 0x10000 then
        return char(0xE0 + floor(cp / 0x1000), 0x80 + floor(cp / 0x40) % 0x40, 0x80 + cp % 0x40)
      end
      return char(0xF0 + floor(cp / 0x40000), 0x80 + floor(cp / 0x1000) % 0x40,
        0x80 + floor(cp / 0x40) % 0x40, 0x80 + cp % 0x40)
    end

    function chars(s)
      local out, k = {}, 0
      for ch in gmatch(s, charpattern) do
        k = k + 1
        out[k] = ch
      end
      return out
    end

    function codepoints(s)
      local out, k, i, n = {}, 0, 1, #s
      while i <= n do
        k = k + 1
        out[k], i = decode(s, i)
      end
      return out
    end

    function bytes(s)
      local out = {}
      for i = 1, #s do out[i] = byte(s, i) end
      return out
    end

    function at(s, k)
      local n = count(s)
      if k < 0 then k = n + k + 1 end
      if k < 1 or k > n then return nil end
      local i = offset(s, k)
      return bsub(s, i, advance(s, i, 1) - 1)
    end

    function slice(s, first, last, exclusive)
      first, last = clamp(first, last, count(s), exclusive)
      if first == nil then return "" end
      local i = offset(s, first)
      return bsub(s, i, advance(s, i, last - first + 1) - 1)
    end

    -- Case mapping is ASCII-only in the stand-in: non-ASCII characters
    -- pass through unchanged.
    upcase, downcase = upper, lower

    function capitalize(s)
      return upper(bsub(s, 1, 1)) .. lower(bsub(s, 2))
    end

    local swap = {}
    for c = 65, 90 do
      swap[char(c)] = char(c + 32)
      swap[char(c + 32)] = char(c)
    end
    function swapcase(s)
      return (gsub(s, "%a", swap))
    end
  end

  I.text = { count = count, offset = offset, advance = advance, charpos = charpos }

  ---------------------------------------------------------------------------
  -- Checks

  -- `s` as a valid UTF-8 string, or raises. `what` names the value for a
  -- message: nil for the receiver, or an argument number. `lib` is the
  -- library the message names, "String" unless given.
  --
  -- `checked` is the last short string found valid. A string checked
  -- again and again, as the one a hot loop iterates over (`for ch in
  -- vowels`), then costs a comparison: `invalid_at` is a C function on
  -- the fork (and a pattern search in the stand-in), and the JIT recorder
  -- ends a trace at every call of one. Short, so that the one string this
  -- keeps alive is never a large one.
  local checked
  local function check(s, fname, what, lib)
    if type(s) ~= "string" then
      rt.typeerror((lib or "String") .. "." .. fname .. (what and (" argument " .. what) or " receiver"), "String", s)
    end
    if s == checked then return s end
    local bad = invalid_at(s)
    if bad ~= nil then
      I.throw("EncodingError", (lib or "String") .. "." .. fname .. ": " .. (what and ("argument " .. what) or "the string")
        .. " is not valid UTF-8 (byte " .. bad .. ")", 3)
    end
    if #s <= 64 then checked = s end
    return s
  end
  I.string_check = check

  -- A string for a byte-level operation: no encoding check.
  local function bytes_arg(s, fname)
    if type(s) ~= "string" then rt.typeerror("String." .. fname .. " receiver", "String", s) end
    return s
  end

  -- An integer argument.
  local function int_arg(v, fname, what, default, lib)
    if v == nil and default ~= nil then return default end
    if type(v) ~= "number" or v ~= floor(v) or v == huge or v == -huge then
      I.throw("TypeError", (lib or "String") .. "." .. fname .. " argument " .. what .. " expects an integer, got "
        .. (type(v) == "number" and tostring(v) or rt.typeof(v)), 3)
    end
    return v
  end
  I.int_arg = int_arg

  -- The Regex library when `v` is a Regex (nl_82_regex.lua), else nil.
  local function regex_arg(v)
    if type(v) == "userdata" and I.isregex ~= nil and I.isregex(v) then return I.Regex end
    return nil
  end

  ---------------------------------------------------------------------------
  -- Characters of a valid string

  function I.chars(s)
    return chars(check(s, "chars"))
  end

  -- `for ch in s`. Invalid UTF-8 raises before the
  -- first character. The iterators are Lua on both targets, so that a
  -- loop over a string can be compiled: the string is valid, so the lead
  -- byte gives each character's length.
  --
  -- I.string_next is the iterator for one loop variable (rt.iter with `n`
  -- 0, nl_40_ops.lua, which checks the string first): the state is the
  -- string, the control the byte where the character before ended, and
  -- the control comes back before the character, so no closure is needed
  -- to keep the position. An ASCII character is taken from a table of the
  -- 128 of them (`ascii[byte + 1]`): `sub` makes a string, which the VM
  -- has to look up among all strings each time, and in a compiled loop
  -- that was most of the cost of a character.
  local ascii = {}
  for c = 0, 127 do ascii[c + 1] = char(c) end
  function I.string_next(s, i)
    i = i + 1
    if i > #s then return nil end
    local c = byte(s, i)
    if c < 0x80 then return i, ascii[c + 1] end
    local last = i + ((c < 0xE0 and 2) or (c < 0xF0 and 3) or 4) - 1
    return last, bsub(s, i, last)
  end

  -- The closure that returns the character alone, for the other forms.
  function I.string_chars(s)
    check(s, "chars")
    local i, n = 1, #s
    return function()
      if i > n then return nil end
      local c = byte(s, i)
      local len = (c < 0x80 and 1) or (c < 0xE0 and 2) or (c < 0xF0 and 3) or 4
      local ch = bsub(s, i, i + len - 1)
      i = i + len
      return ch
    end
  end

  -- `s[i]` and `s[a..b]`, `s[a...b]`: characters by position; negative
  -- positions count from the end. An index of another type raises
  -- TypeError (`level`, counted from here, is the NL code's: rt.index
  -- passes it; an exception names its line through its trace).
  -- The fork's VM calls this for `s[k]` in NL code when `k` is not a
  -- string (LuaJIT/src/lj_meta.c): as a metamethod, so
  -- level 2 is the NL code.
  function rt.strindex(s, k)
    return I.strindex(s, k, 2)
  end

  function I.strindex(s, k, level)
    if type(k) == "number" then
      check(s, "[]")
      if k ~= floor(k) or k == huge or k == -huge then return nil end
      return at(s, k)
    end
    local first, last, exclusive = rt.torange(k)
    if first == nil then
      I.throw("TypeError", "a string is indexed by a number or a range, not " .. rt.typeof(k), level or 2)
    end
    check(s, "[]")
    first, last = int_arg(first, "[]", "1 (range start)"), int_arg(last, "[]", "1 (range end)")
    return slice(s, first, last, exclusive)
  end

  ---------------------------------------------------------------------------
  -- Characters, code points, bytes

  function String.length(s)
    return count(check(s, "length"))
  end

  function String.chars(s)
    return chars(check(s, "chars"))
  end

  function String.codepoints(s)
    return codepoints(check(s, "codepoints"))
  end

  function String.reverse(s)
    local cs = chars(check(s, "reverse"))
    local n = #cs
    for i = 1, floor(n / 2) do cs[i], cs[n - i + 1] = cs[n - i + 1], cs[i] end
    return concat(cs)
  end

  function String.as_bytes(s)
    return bytes(bytes_arg(s, "as_bytes"))
  end

  -- `s.byteslice(i, j)`: bytes `i` to `j`, inclusive, negative from the
  -- end, clamped as in Lua's string.sub. Unspecified: with one number, the
  -- byte at `i` (Ruby's byteslice(i)); a range is accepted too.
  function String.byteslice(s, i, j)
    bytes_arg(s, "byteslice")
    local first, last, exclusive = rt.torange(i)
    if first ~= nil then
      first, last = int_arg(first, "byteslice", "1 (range start)"), int_arg(last, "byteslice", "1 (range end)")
    else
      first = int_arg(i, "byteslice", 1)
      last = int_arg(j, "byteslice", 2, first)
    end
    first, last = clamp(first, last, #s, exclusive)
    if first == nil then return "" end
    return bsub(s, first, last)
  end

  function String.valid_encoding(s)
    return invalid_at(bytes_arg(s, "valid_encoding")) == nil
  end

  -- Unspecified: `"".ord` raises, as in Ruby.
  function String.ord(s)
    check(s, "ord")
    if s == "" then I.throw("Exception", "String.ord: empty string", 2) end
    return (decode(s, 1))
  end

  -- `n.chr` is in the `math` table. Unspecified: a number
  -- that is not a Unicode scalar value (negative, a surrogate, above
  -- U+10FFFF) raises EncodingError; a non-integer raises TypeError.
  function I.chr(n)
    if type(n) ~= "number" or n ~= floor(n) then
      I.throw("TypeError", "Number.chr expects an integer, got "
        .. (type(n) == "number" and tostring(n) or rt.typeof(n)), 2)
    end
    if n < 0 or n > 0x10FFFF or (n >= 0xD800 and n <= 0xDFFF) then
      I.throw("EncodingError", "Number.chr: " .. tostring(n) .. " is not a Unicode code point that UTF-8 can encode", 2)
    end
    return encode(n)
  end
  math.chr = I.chr

  ---------------------------------------------------------------------------
  -- Case (Unicode on the fork, ASCII in the stand-in)

  function String.upcase(s)
    return upcase(check(s, "upcase"))
  end

  function String.downcase(s)
    return downcase(check(s, "downcase"))
  end

  function String.capitalize(s)
    return capitalize(check(s, "capitalize"))
  end

  function String.swapcase(s)
    return swapcase(check(s, "swapcase"))
  end

  ---------------------------------------------------------------------------
  -- Search

  -- `s.index(sub, init = 1)`: the character position of the first
  -- occurrence at or after `init`. Unspecified, as in Ruby: a negative
  -- `init` counts from the end; an `init` before the start or past the end
  -- plus one gives nil; the empty string is found at `init`.
  function String.index(s, pat, init)
    check(s, "index")
    check(pat, "index", 1)
    init = int_arg(init, "index", 2, 1)
    local n = count(s)
    if init < 0 then init = n + init + 1 end
    if init < 1 or init > n + 1 then return nil end
    local b = offset(s, init)
    local a = find(s, pat, b, true)
    if a == nil then return nil end
    return charpos(s, a)
  end

  -- Ruby's: any of several prefixes; none given is false.
  function String.start_with(s, ...)
    check(s, "start_with")
    for k = 1, select("#", ...) do
      local p = check((select(k, ...)), "start_with", k)
      if bsub(s, 1, #p) == p then return true end
    end
    return false
  end

  function String.end_with(s, ...)
    check(s, "end_with")
    for k = 1, select("#", ...) do
      local p = check((select(k, ...)), "end_with", k)
      if p == "" or bsub(s, -#p) == p then return true end
    end
    return false
  end

  ---------------------------------------------------------------------------
  -- Replacement

  -- A replacement string: `$1`, `$name`, `${name}` insert groups of the
  -- match table `m`, `$$` is a dollar sign. These are the rules of the
  -- Rust regex crate: an unbraced name is the
  -- longest run of letters, digits and `_`; a name of digits is a group
  -- number; a group that does not exist or did not participate inserts
  -- nothing; a `$` that starts no reference is kept as it is.
  -- Unspecified: `$0` is the whole match, as `[0]` of the match table (and as in
  -- the Rust crate).
  function I.expand_replacement(rep, m)
    if not find(rep, "$", 1, true) then return rep end
    local out, k, i, n = {}, 0, 1, #rep
    local function put(v)
      k = k + 1
      out[k] = v
    end
    local function group(name)
      local key = name
      if not find(name, "[^0-9]") then key = tonumber(name) end
      local v = m[key]
      if v == nil then return "" end
      return tostring(v)
    end
    while i <= n do
      local d = find(rep, "$", i, true)
      if d == nil then
        put(bsub(rep, i))
        break
      end
      put(bsub(rep, i, d - 1))
      local c = bsub(rep, d + 1, d + 1)
      if c == "$" then
        put("$")
        i = d + 2
      elseif c == "{" then
        local close = find(rep, "}", d + 2, true)
        if close ~= nil and close > d + 2 then
          put(group(bsub(rep, d + 2, close - 1)))
          i = close + 1
        else
          put("$")
          i = d + 1
        end
      else
        local _, e = find(rep, "^[%w_]+", d + 1)
        if e ~= nil then
          put(group(bsub(rep, d + 1, e)))
          i = e + 1
        else
          put("$")
          i = d + 1
        end
      end
    end
    return concat(out)
  end

  -- The replacement for one match, from a string or a function given the
  -- match table `m`. `lib` names the library for messages ("String").
  function I.replacement(rep, m, fname, lib)
    if type(rep) == "string" then
      return I.expand_replacement(rep, m)
    end
    local r = rep(m)
    if type(r) == "number" then return tostring(r) end
    if type(r) ~= "string" then
      I.throw("TypeError", (lib or "String") .. "." .. fname .. ": the replacement function returned "
        .. rt.typeof(r) .. ", not a String", 3)
    end
    -- A character operation gives valid UTF-8: what is inserted is checked.
    local bad = invalid_at(r)
    if bad ~= nil then
      I.throw("EncodingError", (lib or "String") .. "." .. fname .. ": the replacement function returned a string that is"
        .. " not valid UTF-8 (byte " .. bad .. ")", 3)
    end
    return r
  end
  local replacement = I.replacement

  -- The replacement argument: a valid string or a function. `nrep` is its
  -- argument number.
  function I.check_replacement(rep, fname, nrep, lib)
    if type(rep) == "string" then
      check(rep, fname, nrep, lib)
    elseif type(rep) ~= "function" then
      rt.typeerror((lib or "String") .. "." .. fname .. " argument " .. nrep, "String or Function", rep)
    end
  end

  local function replace(s, pat, rep, all, fname)
    check(s, fname)
    local R = regex_arg(pat)
    if R then
      if all then return R.replace_all(pat, s, rep) end
      return R.replace(pat, s, rep)
    end
    check(pat, fname, 1)
    I.check_replacement(rep, fname, 2)
    local out, k = {}, 0
    local function put(v)
      k = k + 1
      out[k] = v
    end
    local i, n = 1, #s
    if pat == "" then
      -- The empty pattern matches at every character boundary.
      while true do
        put(replacement(rep, { [0] = "" }, fname))
        if i > n or not all then break end
        local nxt = advance(s, i, 1)
        put(bsub(s, i, nxt - 1))
        i = nxt
      end
    else
      while true do
        local a, b = find(s, pat, i, true)
        if a == nil then break end
        put(bsub(s, i, a - 1))
        put(replacement(rep, { [0] = pat }, fname))
        i = b + 1
        if not all then break end
      end
    end
    put(bsub(s, i))
    return concat(out)
  end

  function String.sub(s, pat, rep)
    return replace(s, pat, rep, false, "sub")
  end

  function String.gsub(s, pat, rep)
    return replace(s, pat, rep, true, "gsub")
  end

  -- `s.match(pattern, init = 1)`: the match table of the first literal
  -- occurrence at or after character `init`, or nil; a Regex matches.
  function String.match(s, pat, init)
    check(s, "match")
    local R = regex_arg(pat)
    if R then return R.match(pat, s, init) end
    check(pat, "match", 1)
    if String.index(s, pat, init) == nil then return nil end
    return { [0] = pat }
  end

  -- `s.scan(pattern)`: an iterator over the match tables of the literal
  -- occurrences, without overlap; the empty pattern matches at every
  -- character boundary. A Regex scans.
  function String.scan(s, pat)
    check(s, "scan")
    local R = regex_arg(pat)
    if R then return R.scan(pat, s) end
    check(pat, "scan", 1)
    local i, n, done = 1, #s, false
    return function()
      if done then return nil end
      if pat == "" then
        if i > n then done = true else i = advance(s, i, 1) end
        return { [0] = "" }
      end
      local _, b = find(s, pat, i, true)
      if b == nil then
        done = true
        return nil
      end
      i = b + 1
      return { [0] = pat }
    end
  end

  ---------------------------------------------------------------------------
  -- Split, strip, padding

  -- `s.split(sep)`. Unspecified (empty fields): Ruby's String#split. Empty
  -- fields at the start and in the middle are kept; empty fields at the
  -- end are dropped; so the empty string gives an empty list. The empty
  -- separator splits into characters. Without a separator (nil), the
  -- string is split on runs of whitespace (WS) and no field is empty.
  -- Unlike Ruby, a separator of one space is taken literally. A Regex
  -- separator splits at its matches (nl_82_regex.lua).
  function String.split(s, sep)
    check(s, "split")
    local out, k = {}, 0
    local function put(v)
      k = k + 1
      out[k] = v
    end
    if sep == nil then
      for w in gmatch(s, "[^" .. WS .. "]+") do put(w) end
      return out
    end
    local R = regex_arg(sep)
    if R then return R.split(sep, s) end
    check(sep, "split", 1)
    if sep == "" then
      for ch in gmatch(s, charpattern) do put(ch) end
      return out
    end
    local i = 1
    while true do
      local a, b = find(s, sep, i, true)
      if a == nil then break end
      put(bsub(s, i, a - 1))
      i = b + 1
    end
    put(bsub(s, i))
    while k > 0 and out[k] == "" do
      out[k] = nil
      k = k - 1
    end
    return out
  end

  local function lstrip(s)
    local _, e = find(s, "^[" .. WS .. "]*")
    return bsub(s, e + 1)
  end

  local function rstrip(s)
    local j = #s
    while j > 0 and ws_byte[byte(s, j)] do j = j - 1 end
    return bsub(s, 1, j)
  end

  function String.strip(s)
    return rstrip(lstrip(check(s, "strip")))
  end

  function String.lstrip(s)
    return lstrip(check(s, "lstrip"))
  end

  function String.rstrip(s)
    return rstrip(check(s, "rstrip"))
  end

  -- `k` characters of `pad`, repeated from its start.
  local function padding(pad, k)
    local pc = chars(pad)
    local m = #pc
    if m == 1 then return srep(pad, k) end
    return srep(pad, floor(k / m)) .. concat(pc, "", 1, k % m)
  end

  -- Unspecified (padding): Ruby's String#center, ljust, rjust. A width that is
  -- not larger than the length gives the string unchanged; `center` puts
  -- the extra character of an odd padding on the right; the padding
  -- string (default " ") is repeated and cut by characters, and each side
  -- of `center` starts it afresh; an empty padding string raises.
  local function justify(s, width, pad, fname, how)
    check(s, fname)
    width = int_arg(width, fname, 1)
    if pad == nil then pad = " " end
    check(pad, fname, 2)
    if pad == "" then I.throw("Exception", "String." .. fname .. ": the padding string is empty", 3) end
    local total = width - count(s)
    if total <= 0 then return s end
    if how == "left" then return s .. padding(pad, total) end
    if how == "right" then return padding(pad, total) .. s end
    local left = floor(total / 2)
    return padding(pad, left) .. s .. padding(pad, total - left)
  end

  function String.ljust(s, width, pad)
    return justify(s, width, pad, "ljust", "left")
  end

  function String.rjust(s, width, pad)
    return justify(s, width, pad, "rjust", "right")
  end

  function String.center(s, width, pad)
    return justify(s, width, pad, "center", "center")
  end

  ---------------------------------------------------------------------------
  -- Carried over from Lua; they do not depend on the encoding.

  String.format = lstring.format
  String.rep = lstring.rep
end
