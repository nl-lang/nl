-- Changes Lua globals after an NL file has loaded (localize_trade.nl): a
-- library function replaced, and a global that did not exist before.
local upper = string.upper
string.upper = function(s) return "late " .. upper(s) end
LateGlobal = 5
