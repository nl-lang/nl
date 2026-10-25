-- Loading files: `import`, `load_nl`, `include`,
-- `add_path`, and `.nl` files for `require`.
--
-- Fixed names: rt.import, rt.load, rt.addpath. Prelude functions:
-- rt.include, rt.add_path, rt.load_nl. Intrinsics: rt.import_from,
-- rt.owner. For the fork's sandbox: rt.searcher. For hosts that reload
-- changed files: rt.forget.
--
-- One run per state: a file is identified by its canonical
-- name (`host.resolve`), and I.loaded keeps the export table of every file
-- this state has run through `import`, `require` or as the main file
-- (rt.runmain). `include` and `load_nl` do not take part: they run the
-- code every time.
--
-- Import cycles, where a file imports itself, directly or through other
-- files. The export table exists only
-- once the file's main function has returned, so no importer in a cycle
-- could be given it; an import of a file that is still running is an
-- error naming the cycle (the compiler also rejects the cycles it sees
-- without running anything: a file importing itself, and two files
-- importing each other).
do
  -- The sandbox profile (NL_OPT_SANDBOX): host.sandbox(P)
  -- gives P the sandbox's global environment while the option is on, and
  -- host.loadfile loads a Lua file text only into it. Hosts without them
  -- (`nlc run`) have no sandbox.
  local sandbox = host.sandbox
  local loadlua = host.loadfile or loadfile
  -- A bundle's files, compiled ahead of time (host.load), and the host's
  -- way to run a Lua file (host.include).
  local host_load, host_include = host.load, host.include

  -- The second step of the protocol of the header: P(rt) gives the file's
  -- main function. The functions of the file inherit P's environment.
  local function main_of(P)
    if sandbox ~= nil then sandbox(P) end
    local main = P(rt)
    -- A frame of it is "in main chunk" in a trace (nl_50_error.lua).
    if type(main) == "function" then I.mains[main] = true end
    return main
  end

  -- NL source to the file's main function.
  function rt.load(source, chunkname)
    local chunk, msg = host.compile(source, chunkname)
    if chunk == nil then return nil, msg end
    local P, err = loadstring(chunk, chunkname)
    if P == nil then return nil, err end
    return main_of(P)
  end

  -- The main function of the file with canonical name `name`: from
  -- host.load when the host has it, otherwise read and compiled. `why`
  -- ends the message when the file cannot be read (" to import").
  local function load_name(name, why)
    if host_load ~= nil then
      local P, err = host_load(name)
      if P == nil then return nil, err or ("cannot load the file '" .. name .. "'" .. why) end
      return main_of(P)
    end
    local source = host.read(name)
    if source == nil then return nil, "cannot read the file '" .. name .. "'" .. why end
    return rt.load(source, "@" .. name)
  end

  -- `load_nl(source, name)`: like `loadstring`.
  function rt.load_nl(source, name)
    return rt.load(source, "=" .. tostring(name or "load_nl"))
  end

  -- A relative directory is the host's business; `nlc run` takes it
  -- relative to the working directory (crates/nlc/src/host.rs).
  function rt.addpath(dir)
    host.add_path(dir)
  end
  rt.add_path = rt.addpath

  -- `include(path)`: `loadfile`, then a call; the extension
  -- picks the language; nothing is cached. An NL file is named by its
  -- canonical name (host.canonical), as `import` names it, so that one
  -- file has one identity however its path is spelled: its definitions
  -- of primitive methods and operators, and the handler name of its
  -- unnamed `on`, are the same through `include("a.nl")` and
  -- `include("./a.nl")`.
  local canonical = host.canonical

  -- The owner of a loaded chunk, for the rule that a primitive method or
  -- operator from one file cannot be replaced from another
  -- (rt.primmethod, rt.primop). A chunk calls it once per load, with its
  -- chunk name, when it defines one (crates/nl_ir/src/lower/mod.rs). A
  -- file (`@name`) is owned by its canonical name (host.canonical), so a
  -- file has one owner however its name was spelled; any other chunk
  -- (`load_nl`, `=text`) gets an owner of its own, which prints as its
  -- name.
  local anonymous_owner = { __tostring = function(o) return o.name end }
  function rt.owner(chunkname)
    local name = type(chunkname) == "string" and smatch(chunkname, "^@(.+)$") or nil
    if name ~= nil then
      if canonical ~= nil then name = canonical(name) or name end
      return name
    end
    local label = type(chunkname) == "string" and (smatch(chunkname, "^=(.*)$") or chunkname) or "?"
    return setmetatable({ name = label }, anonymous_owner)
  end

  function rt.include(path)
    local f, err
    if type(path) == "string" and ssub(path, -3) == ".nl" then
      local name = path
      if canonical ~= nil then
        name = canonical(path)
        if name == nil then error("cannot read " .. path .. ": no such file", 2) end
      end
      if host_load ~= nil then
        f, err = load_name(name, "")
      else
        local source = host.read(name)
        if source == nil then error("cannot read " .. path, 2) end
        f, err = rt.load(source, "@" .. name)
      end
    elseif host_include ~= nil then
      return host_include(path)
    else
      f, err = loadlua(path)
    end
    if f == nil then error(err, 2) end
    return f()
  end

  -- Files imported by this state, by canonical name: their export tables.
  I.loaded = {}
  -- Who imports whom: importers[name][from] for each import of the file
  -- `name` by the file `from` (canonical names), and the same links the
  -- other way round, imports[from][name], for rt.forget.
  local importers, imports = {}, {}
  -- Files whose main function is running, by canonical name, and the same
  -- names in the order they started (for the message of a cycle).
  local running, running_order = {}, {}

  local function cycle_message(name)
    local start = #running_order
    while start > 1 and running_order[start] ~= name do start = start - 1 end
    local parts = {}
    for i = start, #running_order do parts[#parts + 1] = running_order[i] end
    parts[#parts + 1] = name
    return "import cycle: " .. tconcat(parts, " imports ")
      .. " (a file cannot be imported while it is running)"
  end

  local function finish(name, ok, ...)
    running[name] = nil
    running_order[#running_order] = nil
    if not ok then error((...), 0) end
    return ...
  end

  -- Runs `main` as the file `name`, records its export table and returns
  -- it. An error in the file is raised again as an exception that keeps
  -- the trace of where it happened (rt.handler).
  local function run_as(name, main)
    running[name] = true
    running_order[#running_order + 1] = name
    local exports = finish(name, xpcall(main, rt.handler))
    if exports == nil then exports = {} end
    I.loaded[name] = exports
    return exports
  end

  -- The export table of the file with canonical name `name`, running the
  -- file first if this state has not. Returns nil and a message on
  -- failure to find or compile it.
  local function import_name(name)
    local done = I.loaded[name]
    if done ~= nil then return done end
    if running[name] then return nil, cycle_message(name) end
    local main, err = load_name(name, " to import")
    if main == nil then return nil, err end
    return run_as(name, main)
  end
  I.import_name = import_name

  local function import_path(from, path)
    local name = host.resolve(from, path)
    if name == nil then return nil, "cannot find the file '" .. tostring(path) .. "' to import" end
    if from ~= nil then
      local by = importers[name]
      if by == nil then by = {} importers[name] = by end
      by[from] = true
      local of = imports[from]
      if of == nil then of = {} imports[from] = of end
      of[name] = true
    end
    return import_name(name)
  end

  -- `rt.forget(name)`: the file with canonical name `name` and every file
  -- that imports it, directly or through others, are no longer imported:
  -- the next import of each runs it again, with its source as it is then.
  -- A host that reloads a changed file calls it, then runs again the
  -- files it runs itself (entries) among those returned. Returns the
  -- names, the file's first, then its importers outward. A file that
  -- runs again imports again: the imports it made before are dropped, so
  -- that a file no longer imported does not bring back the one that
  -- stopped importing it.
  function rt.forget(name)
    local out, seen, i = { name }, { [name] = true }, 1
    while out[i] ~= nil do
      local n = out[i]
      I.loaded[n] = nil
      local by = importers[n]
      if by ~= nil then
        local more = {}
        for from in pairs(by) do
          if not seen[from] then seen[from] = true more[#more + 1] = from end
        end
        tsort(more)
        for j = 1, #more do out[#out + 1] = more[j] end
      end
      i = i + 1
    end
    for j = 1, #out do
      local n = out[j]
      local of = imports[n]
      if of ~= nil then
        for dep in pairs(of) do
          local by = importers[dep]
          if by ~= nil then by[n] = nil end
        end
        imports[n] = nil
      end
    end
    return out
  end

  -- Intrinsic of `import` (lowering: crates/nl_ir/src/lower/import.rs).
  -- `from` is the canonical name of the importing file, or nil. With no
  -- names, returns the export table (`import from 'path' as Name`);
  -- otherwise the values of those names in it, in order.
  function rt.import_from(from, path, ...)
    local exports, err = import_path(from, path)
    if exports == nil then error(err, 2) end
    local n = select("#", ...)
    if n == 0 then return exports end
    local names = { ... }
    local values = {}
    for i = 1, n do values[i] = exports[names[i]] end
    return unpack(values, 1, n)
  end

  -- nl_import: `import from 'path' as X` with no importing file.
  function rt.import(path)
    local exports, err = import_path(nil, path)
    if exports == nil then error(err, 2) end
    return exports
  end

  -- The main file of a run counts as imported (one run per state): while
  -- it runs, an import of it is a cycle; afterwards, an import of it gets
  -- its export table. Wraps rt.runmain (nl_50_error.lua).
  local runmain = rt.runmain
  function rt.runmain(main, chunkname)
    I.mains[main] = true
    local name = type(chunkname) == "string" and smatch(chunkname, "^@(.+)$") or nil
    if name == nil or I.loaded[name] ~= nil or running[name] then
      return runmain(main, chunkname)
    end
    return runmain(function() return run_as(name, main) end, chunkname)
  end

  -- `require` finds `.nl` files: a searcher after Lua's
  -- own, so a Lua module of the same name wins as it would without NL.
  -- The module name's dots are directory separators, as for Lua files;
  -- the path resolves with no importing file. The loader returns the
  -- export table, the same one `import` gives, without running the file
  -- a second time.
  -- The sandbox's own `require` uses the same searcher (rt.searcher).
  function rt.searcher(modname)
    if type(modname) ~= "string" then return nil end
    local path = sgsub(modname, "%.", "/")
    local name = host.resolve(nil, path)
    if name == nil then return "\n\tno NL file '" .. path .. ".nl' in the working directory or the NL search path" end
    return function()
      local exports, err = import_name(name)
      if exports == nil then error(err, 2) end
      return exports
    end
  end
  local searchers = package and (package.loaders or package.searchers)
  if searchers then
    searchers[#searchers + 1] = rt.searcher
  end
end
