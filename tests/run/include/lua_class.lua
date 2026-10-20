-- Helper for type_declared_class.nl: a registry of Lua classes that the
-- global `find_class` looks up, as gmod's FindMetaTable does, and that
-- counts the lookups.
local classes = {}
local lookups = 0
function find_class(name)
  lookups = lookups + 1
  return classes[name]
end
return {
  register = function(name)
    local c = {}
    c.__index = c
    -- What getmetatable says of its objects; NL tests them by the real one.
    c.__metatable = "locked"
    classes[name] = c
  end,
  new = function(name, x)
    return setmetatable({ x = x }, classes[name])
  end,
  lookups = function() return lookups end,
}
