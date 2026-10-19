local function here()
  local t = debug.traceback("msg", 1)
  return t
end
local t = here()
print((t:gsub("\n.*", "")))
print(t:match("\n\t([^\n]*)"))
print(debug.traceback("lvl2", 2):match("\n\t([^\n]*)") ~= nil)
local co = coroutine.create(function() end)
print((debug.traceback(co, "other"):gsub("\n\t.*", "")))
print(debug.traceback({}) ~= nil, type(debug.traceback({})))
