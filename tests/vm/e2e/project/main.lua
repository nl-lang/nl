-- A Lua main program: it requires an NL file and includes another.
local greeter = require("nl.greeter")
local main = include("main.nl")
local calc = require("lua.calc")
print(main.summary())
print(greeter.greet("from lua"))
print(calc.greeter() == greeter)
