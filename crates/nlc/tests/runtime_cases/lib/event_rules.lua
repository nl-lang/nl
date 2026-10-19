-- The rules of the `event` stand-in that only Lua code can reach
-- (event_standin.nl).
local event = event
print("package.loaded.event", package.loaded.event == event)

-- Wrong argument types raise as luaL_checkstring and luaL_checktype do.
local function try(f, ...)
  local ok, e = pcall(f, ...)
  print(ok and "ok" or (tostring(e):gsub("^[^:]*:%d+: ", "")))
end
try(event.on_run, {}, "h", function() end)
try(event.on_run, "E", true, function() end)
try(event.on_run, "E", "h", setmetatable({}, { __call = function() end }))
try(event.on_run, "E", "h")
try(event.run, nil)
try(event.run)
try(event.remove, "E", {})

-- Numbers stand for their string form.
event.on_run(5, 1.5, function(x) return "five " .. tostring(x) end)
print(event.run("5", "a"), event.run(5, "b"))
event.remove("5", "1.5")
print("after remove", select("#", event.run(5)))

-- on_run and remove return nothing.
print("on_run returns", select("#", event.on_run("R", "h", function() end)))
print("remove returns", select("#", event.remove("R", "h")))
print("remove of nothing returns", select("#", event.remove("R", "h")))

-- A removed handler's function is released.
local weak = setmetatable({}, { __mode = "v" })
do
  local f = function() end
  weak[1] = f
  event.on_run("Gc", "h", f)
  event.on_run("Gc", "keep", function() end)
end
event.remove("Gc", "h")
collectgarbage("collect")
print("removed function released", weak[1] == nil)

-- An event whose last handler is removed leaves nothing behind.
collectgarbage("collect")
local before = collectgarbage("count")
for i = 1, 20000 do
  local name = "Temp" .. i
  event.on_run(name, "h", function() end)
  event.remove(name, "h")
end
collectgarbage("collect")
collectgarbage("collect")
print("records released", collectgarbage("count") - before < 64)
