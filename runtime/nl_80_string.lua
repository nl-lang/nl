-- NL's string library, the global `String`. Positions are
-- character positions; the text is read as UTF-8.
--
-- Two sets of primitives. On the fork, the
-- host table carries `host.text`, the C glue (LuaJIT/src/lib_nltext.c)
-- over the text library of libnlc (crates/nl_text): validity remembered in
-- the string object, counting and position scans with an ASCII fast path,
-- substrings by character range, Unicode case mapping. Without it (stock
-- LuaJIT: the `nlc run` oracle, portable bundles) the Lua stand-ins below
-- do the same work in Lua; Unicode case mapping needs the host's tables
-- (host.case_tables), without which it is ASCII-only. Everything built on
-- the primitives (search, split, strip, padding, replacement) is the same
-- Lua on both.
--
-- Encoding. The character operations check the whole string first and
-- raise EncodingError for invalid UTF-8 (overlong forms, surrogates, code
-- points above U+10FFFF, truncated or stray bytes). A string argument
-- that they search for or insert (a pattern, a separator, a padding, a
-- replacement, and the string a replacement function returns) is checked
-- too. Once a string is known to be valid, a byte search for a valid
-- needle can only stop at a character boundary, so the searches below are
-- byte searches. That makes some functions byte functions: on valid text
-- and valid arguments their result is the character result, and on any
-- other bytes it is what the same bytes give. These check neither the
-- receiver nor their string arguments, and never raise EncodingError:
--
--   len, <>, as_bytes, byteslice, valid_encoding, scrub (whose
--   replacement is checked), format, rep, fmt, to_number, is_empty,
--   is_blank, start_with, end_with, include, contains, count (but
--   count("") counts characters), remove, delete_prefix, delete_suffix,
--   ensure_prefix, ensure_suffix, chomp, strip, lstrip, rstrip, squish,
--   lines, each_line, indent, dedent, and split with no separator or a
--   non-empty string one (a Regex checks, as the Regex library does).
--
-- Everything that counts characters or maps them (length, index, s[i],
-- case, chop, insert, truncate, padding, wrap, tr, ...) checks. `index`
-- is one of these: its result is a character position, which invalid
-- bytes do not have. There is no lossy mode that reads invalid bytes as
-- characters; `scrub` makes any bytes valid first.
--
-- Regex. A `Regex` argument to `sub`, `gsub`, `split`, `match`, `scan` is
-- handed to the Regex library (nl_82_regex.lua, I.Regex); a string pattern
-- is taken literally. `index`, `start_with`, `end_with`, `include`, `count`
-- and the other searches take strings.
--
-- Keyed options (`s.split(",", limit: 2)`) arrive as the table after the
-- positional arguments (rt.keyed, nl_40_ops.lua); Lua code may pass a
-- plain table there. A key a function does not know raises TypeError.
--
-- Choices where the language is silent are in one place each, marked
-- "unspecified":
--   whitespace (strip, lstrip, rstrip, squish, is_blank, wrap, split
--       without a separator): ASCII space, \t, \n, \v, \f, \r (WS below);
--   s[a..b], byteslice: positions outside the string are clamped, as in
--       Lua's string.sub; an empty range gives "";
--   center, ljust, rjust: Ruby's (see `justify`);
--   split: Ruby's (see String.split);
--   $0 in a replacement: the whole match (see I.expand_replacement);
--   words for the case styles: see `words`;
--   index's `init`, byteslice with one number, "".ord, n.chr outside
--       Unicode, insert's position, fmt's braces, dedent's blank lines,
--       scrub's units: at each function, marked "Unspecified".
--
-- Used by the core and by the Regex library: I.chars, I.string_chars,
-- I.string_next, I.strindex, I.string_check, I.int_arg, I.text (the
-- primitives count, offset, advance, charpos), I.replacement,
-- I.expand_replacement, I.string_opts and I.string_limit (keyed options).
-- I.int_arg is also used by nl_81_number.lua.
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
  -- Patterns over WS: a run of it, leading, and one other character.
  local WS_RUN, WS_LEAD, NOT_WS = "[" .. WS .. "]+", "^[" .. WS .. "]*", "[^" .. WS .. "]"
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

    -- Without the host's tables (below), case mapping is ASCII-only in
    -- the stand-in: non-ASCII characters pass through unchanged.
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

    -- With the host's Unicode tables (host.case_tables, encoded as
    -- crates/nl_text/src/case_tables.rs describes), case mapping is the
    -- text library's: Rust's mappings, one character to one or several.
    -- An ASCII string keeps the ASCII functions above, which give the same
    -- result. The tables are read on first use.
    local tables = type(host) == "table" and type(host.case_tables) == "table" and host.case_tables or nil
    if tables ~= nil then
      local ascii_upcase, ascii_downcase, ascii_capitalize = upcase, downcase, capitalize
      local up, down, swapmap -- character -> its mapping, as strings

      local function hex(x) return tonumber(x, 16) end
      local function read_runs(t, runs)
        for start, n, step, sign, delta in gmatch(runs or "", "(%x+) (%x+) (%x+) ([+-])(%x+);") do
          start, n, step, delta = hex(start), hex(n), hex(step), hex(delta)
          if sign == "-" then delta = -delta end
          for k = 0, n - 1 do
            local c = start + k * step
            t[encode(c)] = encode(c + delta)
          end
        end
      end
      local function read_multi(t, entries)
        for c, to in gmatch(entries or "", "(%x+):([%x.]+);") do
          local parts = {}
          for x in gmatch(to, "%x+") do parts[#parts + 1] = encode(hex(x)) end
          t[encode(hex(c))] = concat(parts)
        end
      end
      local function load()
        up, down, swapmap = {}, {}, {}
        read_runs(up, tables.up)
        read_multi(up, tables.upx)
        read_runs(down, tables.down)
        read_multi(down, tables.downx)
        for k, v in pairs(up) do swapmap[k] = v end
        for k, v in pairs(down) do swapmap[k] = v end
        read_multi(swapmap, tables.swapx)
      end

      local function nonascii(s) return find(s, "[\128-\255]") ~= nil end

      function upcase(s)
        if not nonascii(s) then return ascii_upcase(s) end
        if up == nil then load() end
        return (gsub(s, charpattern, up))
      end

      -- A capital sigma lowercases to the final form when it ends a word
      -- (str::to_lowercase): a cased character comes before it and none
      -- after it, skipping the characters that case ignores. Here a cased
      -- character is one with a case mapping, and the ignored ones are the
      -- usual apostrophes and dots and the combining diacritical marks, a
      -- subset of Unicode's Case_Ignorable.
      local SIGMA = "\206\163"
      local ignorable = { ["'"] = true, ["."] = true, [":"] = true, ["^"] = true, ["`"] = true,
        ["\194\168"] = true, ["\194\173"] = true, ["\194\175"] = true, ["\194\180"] = true,
        ["\194\183"] = true, ["\194\184"] = true, ["\226\128\152"] = true, ["\226\128\153"] = true,
        ["\226\128\164"] = true, ["\226\128\167"] = true }
      local function is_ignorable(ch)
        if ignorable[ch] then return true end
        local cp = decode(ch, 1)
        return cp >= 0x300 and cp <= 0x36F
      end
      local function cased_at(list, i, dir)
        while list[i] ~= nil and is_ignorable(list[i]) do i = i + dir end
        local ch = list[i]
        return ch ~= nil and (up[ch] ~= nil or down[ch] ~= nil)
      end

      function downcase(s)
        if not nonascii(s) then return ascii_downcase(s) end
        if down == nil then load() end
        if not find(s, SIGMA, 1, true) then return (gsub(s, charpattern, down)) end
        local list = chars(s)
        local out = {}
        for i = 1, #list do
          local ch = list[i]
          if ch == SIGMA then
            local final = cased_at(list, i - 1, -1) and not cased_at(list, i + 1, 1)
            out[i] = final and "\207\130" or "\207\131"
          else
            out[i] = down[ch] or ch
          end
        end
        return concat(out)
      end

      function capitalize(s)
        if not nonascii(s) then return ascii_capitalize(s) end
        if up == nil then load() end
        local first = smatch(s, "^" .. charpattern)
        if first == nil then return s end
        return (up[first] or first) .. downcase(bsub(s, #first + 1))
      end

      function swapcase(s)
        if not nonascii(s) then return (gsub(s, "%a", swap)) end
        if up == nil then load() end
        return (gsub(s, charpattern, swapmap))
      end
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

  -- A string for a byte-level operation: no encoding check. `what` as
  -- for check.
  local function bytes_arg(s, fname, what)
    if type(s) ~= "string" then
      rt.typeerror("String." .. fname .. (what and (" argument " .. what) or " receiver"), "String", s)
    end
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
  -- bytes from `i` to the end, as `string.sub(s, i)` (the second number
  -- is an end position, not Ruby's length); a range is accepted too.
  function String.byteslice(s, i, j)
    bytes_arg(s, "byteslice")
    local first, last, exclusive = rt.torange(i)
    if first ~= nil then
      first, last = int_arg(first, "byteslice", "1 (range start)"), int_arg(last, "byteslice", "1 (range end)")
    else
      first = int_arg(i, "byteslice", 1)
      last = int_arg(j, "byteslice", 2, -1)
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

  -- Ruby's: any of several prefixes; none given is false. On bytes (see
  -- the header).
  function String.start_with(s, ...)
    bytes_arg(s, "start_with")
    for k = 1, select("#", ...) do
      local p = bytes_arg((select(k, ...)), "start_with", k)
      if bsub(s, 1, #p) == p then return true end
    end
    return false
  end

  function String.end_with(s, ...)
    bytes_arg(s, "end_with")
    for k = 1, select("#", ...) do
      local p = bytes_arg((select(k, ...)), "end_with", k)
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
  -- Keyed options

  -- The options of a function that takes keyed arguments: `v` when it is a
  -- table (marked by a keyed call, or a plain one from Lua code), else
  -- nil. A key not in `known` raises TypeError.
  local function opts_arg(v, fname, known)
    if type(v) ~= "table" then return nil end
    for k in pairs(v) do
      if not known[k] then
        I.throw("TypeError", "String." .. fname .. " takes no keyed argument '" .. tostring(k) .. "'", 3)
      end
    end
    return v
  end
  I.string_opts = opts_arg

  -- A positive integer `limit` option, or nil.
  local function limit_opt(v, fname)
    if v == nil then return nil end
    v = int_arg(v, fname, "limit")
    if v < 1 then
      I.throw("TypeError", "String." .. fname .. " argument limit expects a positive integer, got " .. tostring(v), 3)
    end
    return v
  end
  I.string_limit = limit_opt

  ---------------------------------------------------------------------------
  -- Split, strip, padding

  -- `s.split(sep, keep_empty: false, limit: nil)`. Unspecified (empty
  -- fields): Ruby's String#split. Empty fields at the start and in the
  -- middle are kept; empty fields at the end are dropped; so the empty
  -- string gives an empty list. The empty separator splits into
  -- characters. Without a separator (nil), the string is split on runs of
  -- whitespace (WS) and no field is empty. Unlike Ruby, a separator of one
  -- space is taken literally. A Regex separator splits at its matches
  -- (nl_82_regex.lua).
  --
  -- `keep_empty: true` keeps every field, as Lua's usual explode does:
  -- "a,,b,".split(",", keep_empty: true) is ["a", "", "b", ""], and the
  -- empty string gives [""]; it changes nothing without a separator.
  -- `limit: n` (Ruby's limit) gives at most n fields, the last one holding
  -- the rest of the string unsplit; with a limit, empty fields at the end
  -- are kept, as in Ruby, but the empty string still gives [] unless
  -- keep_empty is set. Without a separator, the rest starts at the next
  -- field, and a limit of 1 gives the string whole.
  --
  -- On bytes, except for the empty separator, which counts characters.
  local split_opts = { keep_empty = true, limit = true }
  function String.split(s, sep, opts)
    if opts == nil and type(sep) == "table" then sep, opts = nil, sep end
    bytes_arg(s, "split")
    opts = opts_arg(opts, "split", split_opts)
    local keep, limit = false, nil
    if opts ~= nil then
      keep = opts.keep_empty == true
      limit = limit_opt(opts.limit, "split")
    end
    local R = regex_arg(sep)
    if R then return R.split(sep, s, opts) end
    if sep ~= nil then bytes_arg(sep, "split", 1) end
    local out, k = {}, 0
    local function put(v)
      k = k + 1
      out[k] = v
    end
    if s == "" then
      if keep then put("") end
      return out
    end
    if limit == 1 then
      put(s)
      return out
    end
    local n = #s
    if sep == nil then
      local _, e = find(s, WS_LEAD)
      local i = e + 1
      if i > n and limit == nil then return out end
      while true do
        if limit ~= nil and k == limit - 1 then
          put(bsub(s, i))
          break
        end
        local a, b = find(s, WS_RUN, i)
        if a == nil then
          put(bsub(s, i))
          break
        end
        put(bsub(s, i, a - 1))
        i = b + 1
        if i > n then
          if limit ~= nil then put("") end
          break
        end
      end
      return out
    end
    if sep == "" then
      check(s, "split")
      local i = 1
      while i <= n do
        if limit ~= nil and k == limit - 1 then
          put(bsub(s, i))
          break
        end
        local nxt = advance(s, i, 1)
        put(bsub(s, i, nxt - 1))
        i = nxt
      end
      return out
    end
    local i = 1
    while limit == nil or k < limit - 1 do
      local a, b = find(s, sep, i, true)
      if a == nil then break end
      put(bsub(s, i, a - 1))
      i = b + 1
    end
    put(bsub(s, i))
    if not keep and limit == nil then
      while k > 0 and out[k] == "" do
        out[k] = nil
        k = k - 1
      end
    end
    return out
  end

  local function lstrip(s)
    local _, e = find(s, WS_LEAD)
    return bsub(s, e + 1)
  end

  local function rstrip(s)
    local j = #s
    while j > 0 and ws_byte[byte(s, j)] do j = j - 1 end
    return bsub(s, 1, j)
  end

  -- strip, lstrip, rstrip: on bytes (ASCII whitespace only).
  function String.strip(s)
    return rstrip(lstrip(bytes_arg(s, "strip")))
  end

  function String.lstrip(s)
    return lstrip(bytes_arg(s, "lstrip"))
  end

  function String.rstrip(s)
    return rstrip(bytes_arg(s, "rstrip"))
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
  -- Tests, prefixes and suffixes, lines (on bytes: see the header)

  function String.is_empty(s)
    return #bytes_arg(s, "is_empty") == 0
  end

  -- Only whitespace (WS), or nothing.
  function String.is_blank(s)
    return find(bytes_arg(s, "is_blank"), NOT_WS) == nil
  end

  function String.include(s, sub)
    bytes_arg(s, "include")
    return find(s, bytes_arg(sub, "include", 1), 1, true) ~= nil
  end
  String.contains = String.include

  -- The occurrences of `sub`, without overlap. Unlike Ruby's `count`, which
  -- counts the characters of a set, this counts a substring. The empty
  -- string occurs at every character boundary: length + 1.
  function String.count(s, sub)
    bytes_arg(s, "count")
    bytes_arg(sub, "count", 1)
    if sub == "" then return count(check(s, "count")) + 1 end
    local c, i = 0, 1
    while true do
      local _, b = find(s, sub, i, true)
      if b == nil then return c end
      c = c + 1
      i = b + 1
    end
  end

  -- Every occurrence of `sub` removed (Ruby's delete_suffix/delete_prefix
  -- for the ends; `delete` in Ruby removes a set of characters, so this one
  -- is `remove`).
  function String.remove(s, sub)
    bytes_arg(s, "remove")
    bytes_arg(sub, "remove", 1)
    if sub == "" or not find(s, sub, 1, true) then return s end
    local out, k, i = {}, 0, 1
    while true do
      local a, b = find(s, sub, i, true)
      if a == nil then break end
      k = k + 1
      out[k] = bsub(s, i, a - 1)
      i = b + 1
    end
    out[k + 1] = bsub(s, i)
    return concat(out)
  end

  function String.delete_prefix(s, p)
    bytes_arg(s, "delete_prefix")
    bytes_arg(p, "delete_prefix", 1)
    if bsub(s, 1, #p) == p then return bsub(s, #p + 1) end
    return s
  end

  function String.delete_suffix(s, p)
    bytes_arg(s, "delete_suffix")
    bytes_arg(p, "delete_suffix", 1)
    if p ~= "" and bsub(s, -#p) == p then return bsub(s, 1, #s - #p) end
    return s
  end

  function String.ensure_prefix(s, p)
    bytes_arg(s, "ensure_prefix")
    bytes_arg(p, "ensure_prefix", 1)
    if bsub(s, 1, #p) == p then return s end
    return p .. s
  end

  function String.ensure_suffix(s, p)
    bytes_arg(s, "ensure_suffix")
    bytes_arg(p, "ensure_suffix", 1)
    if p == "" or bsub(s, -#p) == p then return s end
    return s .. p
  end

  -- Ruby's chomp: without an argument, one line ending ("\r\n", "\n" or
  -- "\r") is removed from the end; with "", every "\n" and "\r\n" at the
  -- end; with another string, that suffix once.
  function String.chomp(s, suffix)
    bytes_arg(s, "chomp")
    local n = #s
    if suffix == nil then
      local last = byte(s, n)
      if last == 10 then
        if byte(s, n - 1) == 13 then return bsub(s, 1, n - 2) end
        return bsub(s, 1, n - 1)
      elseif last == 13 then
        return bsub(s, 1, n - 1)
      end
      return s
    end
    bytes_arg(suffix, "chomp", 1)
    if suffix == "" then
      while byte(s, n) == 10 do
        n = n - 1
        if byte(s, n) == 13 then n = n - 1 end
      end
      return bsub(s, 1, n)
    end
    if bsub(s, -#suffix) == suffix then return bsub(s, 1, n - #suffix) end
    return s
  end

  -- Ruby's chop: the last character removed, or "\r\n" as one.
  function String.chop(s)
    check(s, "chop")
    local n = #s
    if n == 0 then return s end
    if byte(s, n) == 10 and byte(s, n - 1) == 13 then return bsub(s, 1, n - 2) end
    local i = n
    while i > 1 do
      local c = byte(s, i)
      if c < 0x80 or c >= 0xC0 then break end
      i = i - 1
    end
    return bsub(s, 1, i - 1)
  end

  -- strip, then every run of whitespace (WS) as one space (Rails' squish).
  function String.squish(s)
    return (gsub(rstrip(lstrip(bytes_arg(s, "squish"))), WS_RUN, " "))
  end

  -- The lines without their line endings, as Rust's `lines` (Ruby's keep
  -- them): a line ends at "\n" or "\r\n"; the ending of the last line is
  -- optional, so "a\n" is one line and "" none; a "\r" not followed by
  -- "\n" stays in the line.
  local function lines(s)
    local out, k, i, n = {}, 0, 1, #s
    while i <= n do
      local a = find(s, "\n", i, true)
      k = k + 1
      if a == nil then
        out[k] = bsub(s, i)
        break
      end
      local e = a - 1
      if e >= i and byte(s, e) == 13 then e = e - 1 end
      out[k] = bsub(s, i, e)
      i = a + 1
    end
    return out
  end

  function String.lines(s)
    return lines(bytes_arg(s, "lines"))
  end

  function String.each_line(s, f)
    local ls = lines(bytes_arg(s, "each_line"))
    for i = 1, #ls do f(ls[i]) end
  end

  function String.each_char(s, f)
    local cs = chars(check(s, "each_char"))
    for i = 1, #cs do f(cs[i]) end
  end

  -- `n` copies of `pad` (default one space) before each line that is not
  -- empty, as Rails' indent.
  function String.indent(s, n, pad)
    bytes_arg(s, "indent")
    n = int_arg(n, "indent", 1)
    if pad == nil then pad = " " end
    bytes_arg(pad, "indent", 2)
    if n <= 0 or pad == "" then return s end
    local prefix = srep(pad, n)
    local out, k, i, len = {}, 0, 1, #s
    while i <= len do
      local a = find(s, "\n", i, true) or len
      if a > i or byte(s, a) ~= 10 then
        k = k + 1
        out[k] = prefix
      end
      k = k + 1
      out[k] = bsub(s, i, a)
      i = a + 1
    end
    return concat(out)
  end

  -- The leading spaces and tabs that every line has in common removed, as
  -- Python's textwrap.dedent. Unspecified: a line of only spaces and tabs
  -- does not count, and loses them; the common part is compared byte for
  -- byte, so a tab is not a number of spaces.
  function String.dedent(s)
    bytes_arg(s, "dedent")
    local parts, k, i, len = {}, 0, 1, #s
    while i <= len do
      local a = find(s, "\n", i, true) or len
      k = k + 1
      parts[k] = bsub(s, i, a)
      i = a + 1
    end
    local common
    for j = 1, k do
      local line = parts[j]
      local _, e = find(line, "^[ \t]*")
      local rest = byte(line, e + 1)
      if rest ~= nil and rest ~= 10 and rest ~= 13 then
        local lead = bsub(line, 1, e)
        if common == nil then
          common = lead
        else
          local m = 0
          while m < #common and m < #lead and byte(common, m + 1) == byte(lead, m + 1) do m = m + 1 end
          common = bsub(common, 1, m)
        end
      end
    end
    local cut = common and #common or 0
    for j = 1, k do
      local line = parts[j]
      local _, e = find(line, "^[ \t]*")
      local rest = byte(line, e + 1)
      if rest == nil or rest == 10 or rest == 13 then
        parts[j] = bsub(line, e + 1)
      else
        parts[j] = bsub(line, cut + 1)
      end
    end
    return concat(parts)
  end

  -- Ruby's String#to_i/to_f in one, with Lua's rules: `tonumber(s, base)`,
  -- nil when `s` is not a number. A base is an integer from 2 to 36.
  function String.to_number(s, base)
    bytes_arg(s, "to_number")
    if base == nil then return tonumber(s) end
    base = int_arg(base, "to_number", 1)
    if base < 2 or base > 36 then
      I.throw("TypeError", "String.to_number argument 1 expects a base from 2 to 36, got " .. tostring(base), 2)
    end
    return tonumber(s, base)
  end

  ---------------------------------------------------------------------------
  -- Not valid UTF-8

  -- The length of the maximal subpart at byte `i`, the first byte of an
  -- invalid sequence: the longest start of a well-formed sequence there,
  -- or one byte (Unicode's "substitution of maximal subparts", which is
  -- what Ruby's scrub, Rust's from_utf8_lossy and the WHATWG decoder do).
  local function invalid_length(s, i)
    local c = byte(s, i)
    local len, lo, hi
    if c >= 0xC2 and c <= 0xDF then len, lo, hi = 2, 0x80, 0xBF
    elseif c == 0xE0 then len, lo, hi = 3, 0xA0, 0xBF
    elseif c == 0xED then len, lo, hi = 3, 0x80, 0x9F
    elseif c >= 0xE1 and c <= 0xEF then len, lo, hi = 3, 0x80, 0xBF
    elseif c == 0xF0 then len, lo, hi = 4, 0x90, 0xBF
    elseif c >= 0xF1 and c <= 0xF3 then len, lo, hi = 4, 0x80, 0xBF
    elseif c == 0xF4 then len, lo, hi = 4, 0x80, 0x8F
    else return 1 end
    local c2 = byte(s, i + 1)
    if c2 == nil or c2 < lo or c2 > hi then return 1 end
    local k = 2
    while k < len do
      local ck = byte(s, i + k)
      if ck == nil or ck < 0x80 or ck > 0xBF then break end
      k = k + 1
    end
    return k
  end

  -- `s.scrub(replacement = "\u{FFFD}")`: valid UTF-8, with each invalid
  -- sequence replaced. Unspecified: the unit replaced is the maximal
  -- subpart (invalid_length), so a truncated character is one
  -- replacement and each stray byte is one. A valid string is returned
  -- as it is. Raises nothing for the receiver; the replacement must be
  -- valid.
  function String.scrub(s, rep)
    bytes_arg(s, "scrub")
    if rep == nil then rep = "\239\191\189" else check(rep, "scrub", 1) end
    local bad = invalid_at(s)
    if bad == nil then return s end
    local out, k, i = {}, 0, 1
    while bad ~= nil do
      out[k + 1] = bsub(s, i, bad - 1)
      out[k + 2] = rep
      k = k + 2
      i = bad + invalid_length(s, bad)
      bad = invalid_at(bsub(s, i))
      if bad ~= nil then bad = bad + i - 1 end
    end
    out[k + 1] = bsub(s, i)
    return concat(out)
  end

  ---------------------------------------------------------------------------
  -- Characters: case, positions, sets

  local function first_char(s)
    return smatch(s, "^" .. charpattern) or ""
  end

  -- Rails' upcase_first and downcase_first: the first character only; the
  -- rest is unchanged (capitalize lowercases it).
  function String.upcase_first(s)
    local first = first_char(check(s, "upcase_first"))
    return upcase(first) .. bsub(s, #first + 1)
  end

  function String.downcase_first(s)
    local first = first_char(check(s, "downcase_first"))
    return downcase(first) .. bsub(s, #first + 1)
  end

  -- Case folding for comparisons: the string upcased, then each character
  -- lowercased on its own (so no final sigma), which is Unicode's full
  -- folding for the characters whose mappings the host has: "ß" and "SS"
  -- fold to "ss". Unicode where upcase is (see the top), ASCII otherwise.
  local fold_char = {}
  local function fold(s)
    if not find(s, "[\128-\255]") then return lower(s) end
    return (gsub(upcase(s), charpattern, function(ch)
      local f = fold_char[ch]
      if f == nil then
        f = downcase(ch)
        fold_char[ch] = f
      end
      return f
    end))
  end

  function String.fold(s)
    return fold(check(s, "fold"))
  end

  -- -1, 0 or 1 as `s` sorts before, with or after `other`, compared by
  -- code point after folding (Ruby's casecmp, with Unicode folding).
  function String.casecmp(s, other)
    local a, b = fold(check(s, "casecmp")), fold(check(other, "casecmp", 1))
    if a == b then return 0 end
    return a < b and -1 or 1
  end

  -- Ruby's casecmp?.
  function String.equal_ignoring_case(s, other)
    return fold(check(s, "equal_ignoring_case")) == fold(check(other, "equal_ignoring_case", 1))
  end

  -- `s.insert(i, other)`: `other` before the character at `i`; a negative
  -- `i` counts from the end and inserts after that character, so -1
  -- appends (Ruby's insert, with positions from 1). Unspecified: a
  -- position outside the string is clamped (Ruby raises IndexError).
  function String.insert(s, i, other)
    check(s, "insert")
    i = int_arg(i, "insert", 1)
    check(other, "insert", 2)
    local n = count(s)
    if i < 0 then i = n + i + 2 end
    if i < 1 then i = 1 elseif i > n + 1 then i = n + 1 end
    local b = offset(s, i)
    return bsub(s, 1, b - 1) .. other .. bsub(s, b)
  end

  -- A character set of tr and squeeze, Ruby's syntax: characters, ranges
  -- `a-z`, a leading `^` (when `negate` is allowed and something follows)
  -- for every character but these, and `\` before `-`, `^` or `\` to take
  -- it literally. A range that runs backwards raises. Returns the list of
  -- ranges {lo, hi} of code points, in order, and whether it is negated.
  local function char_set(spec, fname, what, negate)
    local cps = codepoints(check(spec, fname, what))
    local n, i, neg = #cps, 1, false
    if negate and cps[1] == 94 and n > 1 then
      neg = true
      i = 2
    end
    local ranges = {}
    local function at_i()
      local c = cps[i]
      if c == 92 and i < n then
        i = i + 1
        c = cps[i]
      end
      i = i + 1
      return c
    end
    while i <= n do
      local lo = at_i()
      if cps[i] == 45 and i < n then
        i = i + 1
        local hi = at_i()
        if hi < lo then
          I.throw("Exception", "String." .. fname .. ": the range " .. encode(lo) .. "-" .. encode(hi) .. " runs backwards", 3)
        end
        ranges[#ranges + 1] = { lo, hi }
      else
        ranges[#ranges + 1] = { lo, lo }
      end
    end
    return ranges, neg
  end

  local function in_set(ranges, neg, cp)
    for j = 1, #ranges do
      local r = ranges[j]
      if cp >= r[1] and cp <= r[2] then return not neg end
    end
    return neg
  end

  -- Ruby's tr: each character of `from` becomes the character at the same
  -- place in `to`, whose last character repeats when it is shorter; an
  -- empty `to` removes the characters of `from`; with `^from`, every other
  -- character becomes the last of `to`. A character listed twice in `from`
  -- takes its last place.
  function String.tr(s, from, to)
    check(s, "tr")
    local fr, neg = char_set(from, "tr", 1, true)
    local tr = char_set(to, "tr", 2, false)
    local total = 0
    for j = 1, #tr do total = total + tr[j][2] - tr[j][1] + 1 end
    local function to_at(k)
      if k >= total then k = total - 1 end
      for j = 1, #tr do
        local r = tr[j]
        local m = r[2] - r[1] + 1
        if k < m then return r[1] + k end
        k = k - m
      end
    end
    local starts, sum = {}, 0
    for j = 1, #fr do
      starts[j] = sum
      sum = sum + fr[j][2] - fr[j][1] + 1
    end
    local last_to = total > 0 and encode(to_at(total - 1)) or ""
    local memo = {}
    return (gsub(s, charpattern, function(ch)
      local r = memo[ch]
      if r ~= nil then return r end
      local cp = decode(ch, 1)
      r = ch
      if neg then
        if in_set(fr, true, cp) then r = last_to end
      else
        for j = #fr, 1, -1 do
          local g = fr[j]
          if cp >= g[1] and cp <= g[2] then
            r = total > 0 and encode(to_at(starts[j] + cp - g[1])) or ""
            break
          end
        end
      end
      memo[ch] = r
      return r
    end))
  end

  -- Ruby's squeeze: each run of one repeated character as one; with a
  -- set (tr's syntax), only the characters in it.
  function String.squeeze(s, set)
    local cs = chars(check(s, "squeeze"))
    local ranges, neg
    if set ~= nil then ranges, neg = char_set(set, "squeeze", 1, true) end
    local out, k, prev = {}, 0, nil
    for i = 1, #cs do
      local ch = cs[i]
      if ch ~= prev or (ranges ~= nil and not in_set(ranges, neg, (decode(ch, 1)))) then
        k = k + 1
        out[k] = ch
      end
      prev = ch
    end
    return concat(out)
  end

  -- `s.truncate(n, omission: "...", separator: nil)`, Rails' truncate: a
  -- string longer than `n` characters is cut so that, with the omission
  -- after it, it is `n` long; with a separator, it is cut before the last
  -- separator that starts within that length instead, when there is one.
  local truncate_opts = { omission = true, separator = true }
  function String.truncate(s, n, opts)
    check(s, "truncate")
    n = int_arg(n, "truncate", 1)
    opts = opts_arg(opts, "truncate", truncate_opts)
    local omission, sep = "...", nil
    if opts ~= nil then
      if opts.omission ~= nil then omission = check(opts.omission, "truncate", "omission") end
      if opts.separator ~= nil then sep = check(opts.separator, "truncate", "separator") end
    end
    if count(s) <= n then return s end
    local room = n - count(omission)
    if room < 0 then room = 0 end
    local stop = room
    if sep ~= nil and sep ~= "" then
      local last, i = nil, 1
      while true do
        local a = find(s, sep, i, true)
        if a == nil then break end
        local p = charpos(s, a)
        if p > room + 1 then break end
        last = p
        i = a + 1
      end
      if last ~= nil then stop = last - 1 end
    end
    return bsub(s, 1, offset(s, stop + 1) - 1) .. omission
  end

  -- `s.wrap(width, measure: f)`: the lines of a greedy word wrap, each at
  -- most `width` wide where its words allow. Each line of `s` (as `lines`)
  -- is wrapped on its own, and an empty one stays an empty line. Words are
  -- separated by whitespace (WS), which becomes one space within a line
  -- and goes at a break. A word wider than `width` is broken between
  -- characters. `measure(text)` gives the width of a piece of text, by
  -- default its length in characters; a game passes its text-width
  -- function, and the width is then in its units.
  local wrap_opts = { measure = true }
  function String.wrap(s, width, opts)
    check(s, "wrap")
    if type(width) ~= "number" then rt.typeerror("String.wrap argument 1", "Number", width) end
    opts = opts_arg(opts, "wrap", wrap_opts)
    local measure = opts and opts.measure
    if measure == nil then
      measure = count
    elseif type(measure) ~= "function" then
      local mt = getmetatable(measure)
      if type(mt) ~= "table" or rawget(mt, "__call") == nil then
        rt.typeerror("String.wrap argument measure", "Function", measure)
      end
    end
    local function w(t)
      local v = measure(t)
      if type(v) ~= "number" then
        I.throw("TypeError", "String.wrap: measure returned " .. rt.typeof(v) .. ", not a Number", 3)
      end
      return v
    end
    local out, k = {}, 0
    local function put(v)
      k = k + 1
      out[k] = v
    end
    local paras = lines(s)
    for p = 1, #paras do
      local line = nil
      for word in gmatch(paras[p], NOT_WS .. "+") do
        local cand = line and (line .. " " .. word) or word
        if w(cand) <= width then
          line = cand
        else
          if line ~= nil then put(line) end
          line = nil
          if w(word) <= width then
            line = word
          else
            -- Break the word: as many characters as fit, at least one.
            local cs = chars(word)
            local piece = cs[1]
            for c = 2, #cs do
              local more = piece .. cs[c]
              if w(more) <= width then
                piece = more
              else
                put(piece)
                piece = cs[c]
              end
            end
            line = piece
          end
        end
      end
      put(line or "")
    end
    return out
  end

  ---------------------------------------------------------------------------
  -- Case styles

  local function is_upper(ch)
    local c = byte(ch)
    if c < 0x80 then return c >= 65 and c <= 90 end
    return downcase(ch) ~= ch
  end

  local function is_lower(ch)
    local c = byte(ch)
    if c < 0x80 then return c >= 97 and c <= 122 end
    return upcase(ch) ~= ch
  end

  local function is_digit(ch)
    local c = byte(ch)
    return c >= 48 and c <= 57
  end

  -- Unspecified: the words of an identifier or a phrase, for the case
  -- styles. Whitespace (WS), `-` and `_` separate words and are dropped;
  -- a new word also starts at an uppercase letter that follows a
  -- lowercase letter or a digit ("usableItem"), and at the last capital
  -- of a run that a lowercase letter follows ("HTTPServer" is "HTTP",
  -- "Server"), which are the boundaries of Rails' underscore. Other
  -- characters belong to the word they are in. Upper and lower case are
  -- Unicode's where case mapping is.
  local function words(s, fname)
    local cs = chars(check(s, fname))
    local out, cur, m = {}, {}, 0
    local function flush()
      if m > 0 then
        out[#out + 1] = concat(cur, "", 1, m)
        m = 0
      end
    end
    for i = 1, #cs do
      local ch = cs[i]
      if ch == "-" or ch == "_" or (#ch == 1 and ws_byte[byte(ch)]) then
        flush()
      else
        if m > 0 and is_upper(ch) then
          local prev, nxt = cs[i - 1], cs[i + 1]
          if is_lower(prev) or is_digit(prev) or (is_upper(prev) and nxt ~= nil and is_lower(nxt)) then
            flush()
          end
        end
        m = m + 1
        cur[m] = ch
      end
    end
    flush()
    return out
  end

  local function join_words(ws, sep, f, first)
    for i = 1, #ws do ws[i] = (i == 1 and first or f)(ws[i]) end
    return concat(ws, sep)
  end

  -- "UsableItem", "HTTPServer", "usable item" -> "usable_item",
  -- "http_server", "usable_item" (Rails' underscore).
  function String.snake_case(s)
    return join_words(words(s, "snake_case"), "_", downcase)
  end

  function String.kebab_case(s)
    return join_words(words(s, "kebab_case"), "-", downcase)
  end

  -- "usable_item" -> "usableItem". Each word after the first is
  -- capitalized, so the rest of it is lowercased: "HTTPServer" gives
  -- "httpServer".
  function String.camel_case(s)
    return join_words(words(s, "camel_case"), "", capitalize, downcase)
  end

  -- "usable_item" -> "UsableItem"; "HTTPServer" gives "HttpServer".
  function String.pascal_case(s)
    return join_words(words(s, "pascal_case"), "", capitalize)
  end

  -- "man from the boondocks", "x-men" -> "Man From The Boondocks",
  -- "X Men" (Rails' titleize).
  function String.title_case(s)
    return join_words(words(s, "title_case"), " ", capitalize)
  end

  -- `s.parameterize(sep: "-")`, Rails' parameterize: lowercased, each run
  -- of characters other than letters, digits, `-` and `_` replaced by the
  -- separator, repeated separators as one, none at the ends. Unlike
  -- Rails, there is no transliteration: a character beyond ASCII is kept
  -- as a letter (lowercased), so "Héllo Wörld!" gives "héllo-wörld".
  local parameterize_opts = { sep = true }
  function String.parameterize(s, opts)
    check(s, "parameterize")
    opts = opts_arg(opts, "parameterize", parameterize_opts)
    local sep = "-"
    if opts ~= nil and opts.sep ~= nil then sep = check(opts.sep, "parameterize", "sep") end
    local t = gsub(downcase(s), "[^%w\128-\255%-_]+", function() return sep end)
    if sep == "" then return t end
    local twice = sep .. sep
    while find(t, twice, 1, true) do
      t = replace(t, twice, sep, true, "parameterize")
    end
    if bsub(t, 1, #sep) == sep then t = bsub(t, #sep + 1) end
    if bsub(t, -#sep) == sep then t = bsub(t, 1, #t - #sep) end
    return t
  end

  ---------------------------------------------------------------------------
  -- Named placeholders

  -- `s.fmt(name: v, …)`, `s.fmt({name: v})`, `s.fmt(a, b)`: each `{name}`
  -- replaced by its value, as `tostring` gives it, and `{name:spec}` by
  -- `String.format("%spec", value)` (Lua's specifiers: `{n:.2f}`,
  -- `{id:04d}`). A name of digits is a position among the positional
  -- arguments; another name is a keyed argument. One table given alone is
  -- the values, for both kinds of name. `{{` and `}}` are a brace. A name
  -- with no value (nil) raises KeyError.
  -- Unspecified: a brace that starts no placeholder (`{ x }`, `{}`, a
  -- lone `}`) is kept as it is. On bytes: the template is not checked.
  local fmt_spec = "^[-+ #0]*%d*%.?%d*[diouxXeEfgGqsc]$"
  function String.fmt(s, ...)
    bytes_arg(s, "fmt")
    if not find(s, "[{}]") then return s end
    local n = select("#", ...)
    local kw, values
    if n > 0 then
      local last = select(n, ...)
      if type(last) == "table" and getmetatable(last) == I.KW then
        kw = last
        n = n - 1
      end
    end
    local pos = { ... }
    if kw == nil and n == 1 and type(pos[1]) == "table" then values = pos[1] end
    local function value(name)
      local v
      local num = tonumber(name)
      if values ~= nil then
        v = values[num or name]
      elseif num ~= nil then
        if num >= 1 and num <= n then v = pos[num] end
      elseif kw ~= nil then
        v = kw[name]
      end
      if v == nil then
        I.throw("KeyError", "String.fmt: no value for {" .. name .. "}", 3)
      end
      return v
    end
    local out, k, i, len = {}, 0, 1, #s
    local function put(v)
      k = k + 1
      out[k] = v
    end
    while i <= len do
      local a = find(s, "[{}]", i)
      if a == nil then
        put(bsub(s, i))
        break
      end
      put(bsub(s, i, a - 1))
      local c, nxt = byte(s, a, a + 1)
      if c == nxt then
        put(c == 123 and "{" or "}")
        i = a + 2
      elseif c == 123 then
        local name, spec, e = smatch(s, "^{([%w_]+)(:?[^{}]*)}()", a)
        if name == nil or (spec ~= "" and byte(spec) ~= 58) then
          put("{")
          i = a + 1
        else
          local v = value(name)
          if spec == "" then
            put(tostring(v))
          else
            spec = bsub(spec, 2)
            if not find(spec, fmt_spec) then
              I.throw("Exception", "String.fmt: {" .. name .. ":" .. spec .. "} is not a format specifier", 2)
            end
            put(lstring.format("%" .. spec, v))
          end
          i = e
        end
      else
        put("}")
        i = a + 1
      end
    end
    return concat(out)
  end

  ---------------------------------------------------------------------------
  -- Carried over from Lua; they do not depend on the encoding.

  String.format = lstring.format
  String.rep = lstring.rep
end
