-- The Lua side of decl/interop.d.nl, for lua_extend_declared.nl (through
-- import/gadget_extras.nl).
shelf = { count = function() return 3 end }

Gadget = {}
Gadget.__index = Gadget
function Gadget:Label() return "<" .. self.name .. ">" end

function make_gadget(name) return setmetatable({ name = name }, Gadget) end

-- Lua code calls a method NL added as it calls Lua's own.
function lua_shout(g) return g:shout() end

nest = {}
