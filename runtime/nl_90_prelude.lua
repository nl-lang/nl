-- The prelude: the global functions `typeof`, `raise`,
-- `include`, `add_path`, `load_nl`, `keyed`, NL's `String` and `Table`
-- libraries as the globals `String` and `Table`, and the worker library as the global `worker` (also
-- `package.loaded.worker`); the standard file-handle methods marked as
-- methods; NL's String library behind Lua's `string` table; `math` as the
-- `__index` of numbers. A global named in host.skip_globals is left as it
-- is.
do
  local skip = type(host) == "table" and type(host.skip_globals) == "table" and host.skip_globals or {}
  local function global(name, v)
    if not skip[name] then rawset(_G, name, v) end
  end
  global("String", rt.String)
  global("Table", rt.Table)
  global("worker", rt.Worker)
  if type(package) == "table" and type(package.loaded) == "table" and not skip.worker then
    package.loaded.worker = rt.Worker
  end
  global("typeof", rt.typeof)
  global("raise", rt.raise)
  global("include", rt.include)
  global("add_path", rt.add_path)
  global("load_nl", rt.load_nl)
  -- `keyed(t)`: a table as keyed arguments, for Lua code and the C API.
  -- NL code writes `f(**t)`.
  global("keyed", rt.keyed_copy)

  -- `IO.stdout.write(...)` works in NL because the methods of file
  -- handles are marked.
  local stdout = io and io.stdout
  local mt = stdout and getmetatable(stdout)
  local methods = type(mt) == "table" and mt.__index
  if type(methods) == "table" then
    for _, f in pairs(methods) do
      if type(f) == "function" then I.mark(f) end
    end
  end

  -- Lua code reaches NL's String library: `s:to_id()` and
  -- `string.to_id(s)` find a function defined with `fn String.to_id`, and
  -- NL's own (`s:snake_case()`). Strings' `__index` stays Lua's `string`
  -- table, which gets a metatable whose `__index` is NL's library, so
  -- Lua's names are found first and keep Lua's byte semantics, and a hit
  -- costs what it did. Not done when `string` already has a metatable,
  -- nor when host.string_methods is false.
  if not (type(host) == "table" and host.string_methods == false) and getmetatable(lstring) == nil then
    setmetatable(lstring, { __index = rt.String })
  end

  -- Lua code reaches Number methods on every number. Needs
  -- the debug library, which an embedder may not open.
  local dsm = debug_setmetatable
  if dsm then
    pcall(dsm, 0, { __index = math })
    I.setmetatable_bool = function()
      pcall(dsm, true, { __index = I.Bool })
      I.setmetatable_bool = nil
    end
  end
end
