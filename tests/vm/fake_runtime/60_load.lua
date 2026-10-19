-- Loading (fake): import caches by canonical name; load uses host.compile and
-- the two-step chunk protocol of crates/nl_capi/src/driver.rs.

local loaded = {}

function rt.load(source, chunkname)
  local chunk, msg = host.compile(source, chunkname)
  if not chunk then return nil, msg end
  local P, err = loadstring(chunk, chunkname)
  if not P then return nil, err end
  return P(rt)
end

function rt.import(path, from)
  local name = host.resolve(from, path)
  if not name then rt.raise("cannot find '" .. path .. "'") end
  local exports = loaded[name]
  if exports == nil then
    local src = host.read(name)
    if not src then rt.raise("cannot read '" .. name .. "'") end
    local main, msg = rt.load(src, "@" .. name)
    if not main then rt.raise(msg) end
    exports = main() or true
    loaded[name] = exports
  end
  return exports
end

function rt.addpath(dir) host.add_path(dir) end

-- Prelude globals, minus include, which the C side provides.
typeof = rt.typeof
raise = rt.raise
add_path = rt.addpath
load_nl = rt.load

-- The .nl package searcher.
if package and package.loaders then
  table.insert(package.loaders, 2, function(modname)
    local name = host.resolve(nil, (modname:gsub("%.", "/")))
    if not name then return "\n\tno NL file for '" .. modname .. "'" end
    return function() return rt.import((modname:gsub("%.", "/"))) end
  end)
end
