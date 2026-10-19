-- A Lua module that requires an NL file through the .nl package searcher.
local greeter = require("nl.greeter")
local M = {}
function M.add(a, b) return a + b end
function M.from_nl(who) return greeter.greet(who) end
function M.greeter() return greeter end
return M
