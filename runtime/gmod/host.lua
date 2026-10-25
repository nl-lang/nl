-- The host of the NL runtime in Garry's Mod, on gmod's stock LuaJIT (2.0
-- or 2.1, client or server), for portable bundles (`nlc bundle`). It is
-- not part of the runtime chunk (runtime/*.lua): a bundle carries it and
-- runs it once per Lua state, before the runtime:
--
--   local host, start = <this chunk>()
--   host.case_tables = { … }            -- when the bundle has them
--   local rt = <runtime chunk>(host)
--   start(rt)
--
-- `host` is the host table of runtime/nl.lua ("HOST TABLE"), with no
-- compiler: a bundle's files arrive compiled, as Lua source, and are
-- loaded by name (host.load). `start(rt)` connects the runtime to the
-- game and adds `rt.run_bundle`.
--
-- What it takes from gmod, each read where it is used, so that a detour
-- installed by another addon later is honored (the functions of `hook`
-- and `timer` are commonly replaced):
--
--   hook.Add, hook.Remove, hook.Run   NL's event library (below)
--   hook.Add("Think", …)              the pump: queued callbacks and
--                                     finished jobs run once per frame
--   timer.Simple                      Task.sleep
--   CompileString                     loading the bundle's files, where
--                                     `loadstring` is absent
--   include                           NL's `include` of a Lua file
--   ErrorNoHalt                       task failures that nothing handles
--   FindMetaTable                     the declared host types (below)
--
-- Each is optional: without `hook` the event library is the runtime's
-- own stand-in and nothing pumps, without `timer` Task.sleep raises, and
-- so on, so the same bundle also runs on a bare stock LuaJIT whose
-- embedder pumps.
--
-- EVENTS. NL's event library (`Event`, `on`) is built on gmod's `hook`
-- library, so NL handlers and Lua ones are one set: the engine's events
-- reach `on` handlers, `hook.Run` from Lua reaches them, and Lua code sees
-- them in `hook.GetTable()` and can remove them by name.
--
--   Event.on_run(name, handler, f)  hook.Add(name, handler, f): the
--                                   handler name is the hook identifier
--                                   (an unnamed `on` uses its file's
--                                   canonical name, as everywhere)
--   Event.remove(name, handler)     hook.Remove(name, handler)
--   Event.run(name, ...)            hook.Run(name, ...)
--
-- The arguments are checked as the runtime's own library checks them
-- (names are strings, a number stands for its string form, the handler
-- is a function), and a bad one raises. Everything else is the hook
-- library's: handlers run in its order, which is not the order they were
-- added; a handler that returns nil as its first value does not end the
-- run; a run returns at most six values; when no handler returns a
-- value, Event.run calls the gamemode's function of the event; a handler
-- added or removed during a run follows the rules of `pairs`.
do
  local G = _G
  local type, tostring, select, rawget, error, pairs, pcall = type, tostring, select, rawget, error, pairs, pcall
  local unpack = unpack or table.unpack
  local gmatch, gsub, concat = string.gmatch, string.gsub, table.concat
  local huge = math.huge

  local function global(name)
    return rawget(G, name)
  end

  -- The bundles' files: canonical name -> Lua source, and how their
  -- imports resolve: from .. "\0" .. path -> canonical name (from is ""
  -- for none). Shared by every bundle of the state.
  local sources, resolved = {}, {}
  local register_types

  local host = {}

  host.skip_globals = { include = true, load_nl = true, add_path = true }

  local function no_compiler()
    return "there is no NL compiler in this Lua state: NL files must be compiled into a bundle (nlc bundle)"
  end

  function host.compile()
    return nil, no_compiler()
  end

  function host.read()
    return nil
  end

  function host.add_path()
    error("add_path: " .. no_compiler(), 3)
  end

  -- Lexical: removes `./` and resolves `x/..`.
  local function normalize(path)
    local parts = {}
    for part in gmatch(path, "[^/]+") do
      if part == ".." and #parts > 0 and parts[#parts] ~= ".." then
        parts[#parts] = nil
      elseif part ~= "." then
        parts[#parts + 1] = part
      end
    end
    return concat(parts, "/")
  end

  function host.resolve(from, path)
    if type(path) ~= "string" then return nil end
    local name = resolved[(from or "") .. "\0" .. path]
    if name ~= nil or from ~= nil then return name end
    -- No importing file (rt.import, `require`): a file of a bundle by its
    -- canonical name, with or without its extension.
    local p = normalize(path)
    if sources[p] ~= nil then return p end
    if sources[p .. ".nl"] ~= nil then return p .. ".nl" end
    return nil
  end

  function host.canonical(path)
    local p = normalize(path)
    if sources[p] ~= nil then return p end
    return nil
  end

  -- Lua's `loadstring`, or gmod's `CompileString`, which returns the
  -- message instead of raising when its third argument is false.
  local loadstring = global("loadstring")
  local function compile_lua(source, chunkname)
    if loadstring ~= nil then return loadstring(source, chunkname) end
    local CompileString = global("CompileString")
    if CompileString == nil then return nil, "neither loadstring nor CompileString is available" end
    -- gmod names the chunk after the identifier as it is: drop Lua's
    -- `@`/`=` prefix.
    local f = CompileString(source, (gsub(chunkname, "^[@=]", "")), false)
    if type(f) ~= "function" then return nil, tostring(f) end
    return f
  end
  if loadstring == nil then host.loadstring = compile_lua end

  function host.load(name)
    local source = sources[name]
    if source == nil then
      return nil, "the file '" .. tostring(name) .. "' is not in a bundle loaded in this Lua state; " .. no_compiler()
    end
    return compile_lua(source, "@" .. name)
  end

  if global("include") ~= nil then
    host.include = function(path) return global("include")(path) end
  end

  -- The event library over `hook` (see EVENTS above).
  if type(global("hook")) == "table" then
    local event = {}

    local function checkname(v, n, fname, nargs)
      local tv = type(v)
      if tv == "string" then return v end
      if tv == "number" then return tostring(v) end
      error("bad argument #" .. n .. " to '" .. fname .. "' (string expected, got "
        .. (n > nargs and "no value" or tv) .. ")", 3)
    end

    function event.on_run(...)
      local nargs = select("#", ...)
      local name, handler, f = ...
      name = checkname(name, 1, "on_run", nargs)
      handler = checkname(handler, 2, "on_run", nargs)
      if type(f) ~= "function" then
        error("bad argument #3 to 'on_run' (function expected, got " .. (nargs < 3 and "no value" or type(f)) .. ")", 2)
      end
      global("hook").Add(name, handler, f)
    end

    function event.remove(...)
      local nargs = select("#", ...)
      local name, handler = ...
      name = checkname(name, 1, "remove", nargs)
      handler = checkname(handler, 2, "remove", nargs)
      global("hook").Remove(name, handler)
    end

    function event.run(...)
      local name = checkname((...), 1, "run", select("#", ...))
      return global("hook").Run(name, select(2, ...))
    end

    host.event = event
  end

  -- Host types. A declaration file's `declare type Player : Entity`
  -- makes a check against `Player` look the type up by name at run time
  -- (rt.gettype), so the host registers each declared type that is one of
  -- gmod's classes against the class's metatable, `FindMetaTable(name)`,
  -- with its parent's metatable, as rt.settype(mt, name, parent_mt). A
  -- bundle lists its declared types, parents first; a type already
  -- registered is left alone. What has no metatable of its own stays
  -- unregistered, and a check against it fails: the derma classes, whose
  -- panels all carry gmod's one `Panel` metatable, and the tables of hooks
  -- (`GM`, `ENTITY`, …), which are plain tables.
  --
  -- The NL names of two classes differ from gmod's, because a built-in
  -- declaration has gmod's name.
  local class_names = { FileHandle = "File", NPCTask = "Task" }

  register_types = function(rt, types)
    local FindMetaTable = global("FindMetaTable")
    if type(types) ~= "table" or FindMetaTable == nil then return end
    for i = 1, #types do
      local name, parent = types[i][1], types[i][2]
      if rt.gettype(name) == nil then
        local ok, mt = pcall(FindMetaTable, class_names[name] or name)
        local parent_mt = parent ~= nil and rt.gettype(parent) or nil
        if ok and type(mt) == "table" and (parent == nil or parent_mt ~= nil) then
          -- A metatable that is already a type under another name stays so.
          pcall(rt.settype, mt, name, parent_mt)
        end
      end
    end
  end

  -- After the runtime has opened.
  local function start(rt)
    -- Task.sleep: one gmod timer per sleep. A sleep of infinity never
    -- ends; NaN and a negative time end at the next frame.
    if type(global("timer")) == "table" then
      rt.settimer(function(seconds, wake)
        if seconds == huge then return end
        if not (seconds > 0) then seconds = 0 end
        global("timer").Simple(seconds, wake)
      end)
    end

    -- The pump, once per frame. What a pumped callback queues runs in
    -- the same frame, up to a bound, so that a chain of resumed tasks
    -- does not take a frame per step and a callback that queues itself
    -- does not hold the frame. On the fork (the server of lua_shared)
    -- rt.pumpall is nl_pump, which also delivers what workers send back
    -- and steps the workers that have no thread. Returns nothing: a Think
    -- handler that returned a value would end the run of the other Think
    -- handlers.
    local hook = global("hook")
    if type(hook) == "table" then
      local pump = rt.pumpall or rt.pump
      hook.Add("Think", "NL.pump", function()
        for _ = 1, 64 do
          if not pump() then break end
        end
      end)
    end

    -- A failure that nothing handles is reported as gmod reports errors
    -- that do not stop the code: in the console, in red, without a halt.
    local ErrorNoHalt = global("ErrorNoHalt")
    if ErrorNoHalt ~= nil then
      rt.setunhandled(function(e)
        local trace = type(e) == "table" and rawget(e, "trace") or nil
        ErrorNoHalt("unhandled task failure: " .. rt.tostring(e) .. "\n" .. (trace and (tostring(trace) .. "\n") or ""))
      end)
    end

    -- rt.run_bundle(files, imports, entries, types): adds a bundle's
    -- files (canonical name -> Lua source) and the resolution of their
    -- imports (from .. "\0" .. path -> canonical name), registers the
    -- host types of its declaration files (register_types), then runs
    -- each entry, in order, as `import` would: a file that has run in this
    -- state already is not run again, unless the bundle brings it, or a
    -- file it imports, with a source that changed since (gmod's
    -- autorefresh sends a client the new bundle of a changed file): then
    -- it runs again (rt.forget). Returns the export table of each entry.
    -- An error is raised as text, with the NL trace.
    function rt.run_bundle(files, imports, entries, types)
      local changed = {}
      for name, source in pairs(files) do
        if sources[name] ~= nil and sources[name] ~= source then changed[#changed + 1] = name end
        sources[name] = source
      end
      for i = 1, #changed do rt.forget(changed[i]) end
      for key, name in pairs(imports) do resolved[key] = name end
      register_types(rt, types)
      local results = {}
      for i = 1, #entries do
        local name = entries[i]
        results[i] = rt.runmain(function() return rt.import(name) end, "=" .. name)
      end
      return unpack(results, 1, #entries)
    end
  end

  return host, start
end
