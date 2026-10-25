-- Exceptions: the built-in hierarchy, `raise`, the
-- message handler that wraps foreign errors, and the translator hook.
--
-- An exception is a struct instance; its struct has a `__tostring`, so Lua
-- code that prints a caught NL error sees "Type: message".
--
-- Fixed names: rt.raise, rt.handler, rt.exception, rt.settranslator.
-- Intrinsics: the exception structs (rt.Exception, rt.TypeError, …);
-- `try` support goes here (rt.xpcall, rt.rethrow, rt.try); rt.linemap.
--
-- TRACES. `trace` lists the frames of the
-- program, innermost first, under a "stack traceback:" line:
--
--   stack traceback:
--   	a.nl:2: in function 'inner'          a function of the name registry
--   	a.nl:11: in function <a.nl:10>       any other function, by where it starts
--   	a.nl:10: in main chunk               a file's main function
--
-- Frames that are not the program's are left out: C functions (`error`,
-- `pcall`, library functions) and the chunks of the runtime library and
-- of the embedder, whose names start with "=nl " or "=[nl " ("=nl runtime"
-- and "=nl host" in `nlc run`; "=[nl runtime]" and "=[nl event]" on the
-- fork, LuaJIT/src/Makefile and lib_event.c). So the first frame
-- is the one that raised, or the frame of the program that called the
-- library function that did. The rule reads only chunk names and the
-- standard debug fields, so a trace is the same on stock LuaJIT and on
-- the fork. The frames come from `debug.getinfo`; without it, from the
-- text of `debug.traceback`, filtered by the same rule; without either,
-- there is no trace. At most 24 frames are listed, then "...", and at
-- most 64 stack levels are looked at: a trace is built for every caught
-- error, so its cost is bounded.
--
-- A frame shows the file and the NL line: a chunk from the Lua backend
-- may carry a line map (rt.linemap, crates/nl_luagen), applied to the
-- frames' lines and to the `chunk:line:` prefix of an error message that
-- becomes a LuaError's message.
do
  local S = rt.T.String

  local function exc_tostring(e)
    local info = I.info[getmetatable(e)]
    local msg = rawget(e, "message")
    return (info and info.name or "Exception") .. ": " .. tostring(msg)
  end

  rt.Exception = rt.struct("Exception", nil, {
    { name = "message", type = function() return S end, default = function() return "" end },
    { name = "trace", type = function() return rt.optional(S) end },
  }, "=nl runtime")
  rt.Exception.__tostring = exc_tostring

  local function sub(name, extra)
    rt[name] = rt.struct(name, rt.Exception, extra or {}, "=nl runtime")
  end
  -- LuaError keeps the original error value in `value`.
  sub("LuaError", { { name = "value" } })
  sub("TypeError")
  sub("MatchError")
  sub("ConversionError")
  sub("RegexError")
  sub("EncodingError")
  -- A name with no value (String.fmt), as Ruby's KeyError.
  sub("KeyError")
  sub("SendError")
  sub("WorkerError")

  local function is_exception(v)
    return rt.isa(v, rt.Exception)
  end
  I.is_exception = is_exception

  -- Raises a built-in exception from the runtime. `level` is unused: an
  -- exception carries no position in its message; its `trace` is filled
  -- in when it is caught.
  function I.throw(typename, message, level)
    local _ = level
    error(rt.new(rt[typename] or rt.Exception, { message = message }), 0)
  end

  -- `raise(e)`: a string becomes `Exception { message: e }`.
  function rt.raise(e)
    if type(e) == "string" then
      e = rt.new(rt.Exception, { message = e })
    end
    error(e, 0)
  end

  -- nlL_raise without raising: `typename` names a type known to
  -- rt.gettype that is an Exception (nil is Exception); any other name is
  -- an error, as nl.h allows only those.
  function rt.exception(typename, message)
    local S2 = rt.Exception
    if typename ~= nil then
      S2 = rt.gettype(typename)
      if S2 == nil then
        error("exception: no exception type named " .. tostring(typename) .. " is known (nl_gettype)", 2)
      end
      local info = I.info[S2]
      if not info or info.kind ~= "struct" or not info.ancestors[rt.Exception] then
        error("exception: " .. tostring(typename) .. " is not an exception type", 2)
      end
    end
    return rt.new(S2, { message = message or "" })
  end

  -- Line maps of loaded chunks: chunk name -> { [chunk line] = NL line },
  -- and the same maps by the chunk name without its "@" or "=", the name
  -- an error message's prefix shows.
  local linemaps, linemaps_short = {}, {}

  -- rt.linemap(chunkname, { line1, nl1, line2, nl2, ... }): called once
  -- per load by a chunk whose lines are not all NL lines (the module doc
  -- of crates/nl_luagen). A chunk loaded again replaces its map.
  function rt.linemap(chunkname, list)
    local m = {}
    for i = 1, #list - 1, 2 do m[list[i]] = list[i + 1] end
    linemaps[chunkname] = m
    local c = ssub(chunkname, 1, 1)
    if c == "@" or c == "=" then linemaps_short[ssub(chunkname, 2)] = m end
  end

  -- An error message with its `chunk:line:` prefix mapped to the NL line.
  local function map_message(msg)
    local src, line, at = smatch(msg, "^([^\n]-):(%d+): ()")
    if src == nil then return msg end
    local m = linemaps_short[src]
    local nl = m and m[tonumber(line)]
    if nl == nil then return msg end
    return src .. ":" .. nl .. ": " .. ssub(msg, at)
  end

  -- The main functions of loaded files (rt.load, rt.runmain): a frame of
  -- one is "in main chunk".
  local mains = setmetatable({}, { __mode = "k" })
  I.mains = mains

  local getinfo = ldebug.getinfo
  local sgmatch = lstring.gmatch

  -- Whether a chunk name is the runtime's or the embedder's (see TRACES).
  local function foreign_source(s)
    -- nlc names its chunks `=nl runtime` and `=nl host…`; the fork's
    -- bytecode dumps are `@=[nl runtime]` and `@=[nl event]` (bcsave -F
    -- keeps the `=` and adds `@`), and the traceback fallback hands in
    -- short names with one more `=`.
    return s == "=[C]" or smatch(s, "^[@=]+%[?nl ") ~= nil
  end

  local function frame_text(info)
    local s = info.source or "?"
    local c = ssub(s, 1, 1)
    local shown = (c == "@" or c == "=") and ssub(s, 2) or info.short_src or "?"
    local m = linemaps[s]
    local line = info.currentline or -1
    if m and m[line] then line = m[line] end
    local where = line > 0 and (shown .. ":" .. line .. ": ") or (shown .. ": ")
    local f = info.func
    local named = f ~= nil and I.named and I.named[f]
    if named then return where .. "in function '" .. tostring(named[2]) .. "'" end
    if (f ~= nil and mains[f]) or info.what == "main" then return where .. "in main chunk" end
    local def = info.linedefined or 0
    if m and m[def] then def = m[def] end
    return where .. "in function <" .. shown .. ":" .. def .. ">"
  end

  local MAX_FRAMES, MAX_LEVELS = 24, 64

  -- The trace of the stack of the caller, by the rule of TRACES.
  local function build_trace()
    local lines = { "stack traceback:" }
    if getinfo ~= nil then
      local n = 0
      for level = 2, MAX_LEVELS + 1 do
        local info = getinfo(level, "Slf")
        if info == nil then break end
        if info.what ~= "C" and not foreign_source(info.source or "") then
          if n == MAX_FRAMES then lines[#lines + 1] = "\t..."; break end
          n = n + 1
          lines[#lines + 1] = "\t" .. frame_text(info)
        end
        if level == MAX_LEVELS + 1 and getinfo(level + 1, "S") ~= nil then lines[#lines + 1] = "\t..." end
      end
      return tconcat(lines, "\n")
    end
    if traceback == nil then return nil end
    local ok, tb = pcall(traceback, "", 2)
    if not ok or type(tb) ~= "string" then return nil end
    local n = 0
    for line in sgmatch(tb, "[^\n]+") do
      local src, num, rest = smatch(line, "^\t([^:]*):(%d*):? ?(.*)$")
      if src ~= nil and src ~= "[C]" and not foreign_source("=" .. src) then
        if n == MAX_FRAMES then lines[#lines + 1] = "\t..."; break end
        n = n + 1
        local m = linemaps_short[src]
        local nl = num ~= "" and m and m[tonumber(num)] or nil
        lines[#lines + 1] = "\t" .. src .. ":" .. (nl or num) .. (num ~= "" and ": " or " ") .. rest
      end
    end
    return tconcat(lines, "\n")
  end

  local translator = nil
  function rt.settranslator(f)
    translator = f
  end

  -- The text of an error value for LuaError's message. A `__tostring` that
  -- raises or returns something other than a string does not stop the
  -- value from being caught.
  local function describe(e)
    if type(e) == "string" then return e end
    local ok, text = pcall(tostring, e)
    if ok and type(text) == "string" then return text end
    return "a " .. rt.typeof(e) .. " error value (its __tostring "
      .. (ok and ("returned " .. rt.typeof(text)) or "raised an error") .. ")"
  end
  I.describe = describe

  -- The message handler of `try` and nl_pcall: every caught
  -- value becomes an Exception; a value that is not one is first offered
  -- to the host's translator, then wrapped in a LuaError, whose message
  -- has its position mapped to the NL line. The trace is filled in here,
  -- where the stack is still intact (see TRACES).
  function rt.handler(e)
    if not is_exception(e) and translator ~= nil then
      local ok, t = pcall(translator, e)
      if ok and is_exception(t) then e = t end
    end
    if not is_exception(e) then
      e = rt.new(rt.LuaError, { message = map_message(describe(e)), value = e })
    end
    if rawget(e, "trace") == nil then
      local ok, tb = pcall(build_trace)
      if ok and type(tb) == "string" then rawset(e, "trace", tb) end
    end
    return e
  end

  local function pack(...) return { n = select("#", ...), ... } end

  -- `try`, lowered by the compiler's `error` module. Its
  -- closures return a status code and values ("n" normal, "r" return,
  -- "b" break, "c" continue; the catch closure gives "u" when no clause
  -- matched). Most `try` statements run the closures themselves, as
  -- `rt.xpcall(f, rt.handler, args...)` (`xpcall` as the runtime took it,
  -- which a script cannot replace), and raise an error that goes on with
  -- `rt.rethrow`, which raises the value as it is.
  rt.xpcall = xpcall
  function rt.rethrow(e)
    error(e, 0)
  end

  -- The others, whose closures return varying numbers of values, call
  -- `rt.try`. The arguments after `finally` are passed to each closure
  -- (after the error, for `catch`). Returns { true, code, values..., n =
  -- count }; an error that no clause caught, or one raised by a catch
  -- clause, is raised again after `finally` has run. A `finally` that
  -- leaves by `return`, `break` or `continue` overrides the outcome.
  function rt.try(body, catch, finally, ...)
    local r = pack(xpcall(body, rt.handler, ...))
    if not r[1] and catch ~= nil then
      local e = r[2]
      r = pack(xpcall(catch, rt.handler, e, ...))
      if r[1] and r[2] == "u" then r = { false, e, n = 2 } end
    end
    if finally ~= nil then
      local f = pack(xpcall(finally, rt.handler, ...))
      if not f[1] then error(f[2], 0) end
      if f[2] ~= "n" then return f end
    end
    if not r[1] then error(r[2], 0) end
    return r
  end

  -- Runs a file's main function. An uncaught error is re-raised as text
  -- with its trace, for the embedder to report.
  function rt.runmain(main, chunkname)
    mains[main] = true
    local ok, e = xpcall(main, rt.handler)
    if ok then return e end
    local text = describe(e)
    if is_exception(e) and rt.typeof(e) == "LuaError" then text = describe(rawget(e, "message")) end
    local trace = is_exception(e) and rawget(e, "trace") or nil
    error((chunkname and (sgsub(tostring(chunkname), "^[@=]", "") .. ": ") or "") .. "uncaught " .. text
      .. (trace and ("\n" .. trace) or ""), 0)
  end
end
