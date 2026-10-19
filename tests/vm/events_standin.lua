-- Runs test_events.lua against the Lua stand-in of the event library
-- (runtime/nl_62_event.lua) on stock LuaJIT:
--   <stock luajit> tests/vm/events_standin.lua   (from the repository root)
-- The runtime chunk is the concatenation of runtime/*.lua (in name order,
-- as `ls` sorts them in the C locale); it installs the stand-in because the
-- global `event` is absent on stock LuaJIT.
assert(event == nil, "run this on stock LuaJIT: `event` already exists")
local p = io.popen("LC_ALL=C ls runtime/*.lua")
local parts = {}
for f in p:lines() do
  local h = assert(io.open(f, "rb")); parts[#parts + 1] = h:read("*a"); h:close()
  if parts[#parts]:sub(-1) ~= "\n" then parts[#parts] = parts[#parts] .. "\n" end
end
p:close()
local host = {
  compile = function() return nil, "no compiler here" end,
  resolve = function() return nil end,
  read = function() return nil end,
  add_path = function() end,
}
assert(loadstring(table.concat(parts), "=nl runtime"))(host)
assert(type(event) == "table", "the runtime did not install the stand-in")
dofile("tests/vm/test_events.lua")
