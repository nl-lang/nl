local rt = assert(NL_RT, "run with NLC_TEST_RT_GLOBAL=NL_RT")
local function try(label, f, ...)
  local ok, err = pcall(f, ...)
  print(label, ok and "ok" or tostring(err))
end
for _, n in ipairs({ "Exception", "LuaError", "TypeError", "MatchError", "ConversionError", "RegexError",
                     "EncodingError", "SendError", "WorkerError", "Range", "Task", "Regex" }) do
  print("gettype", n, rt.gettype(n) ~= nil)
end
for _, n in ipairs({ "Number", "Nil", "Any", "To", "check", "Sprite" }) do
  print("gettype", n, rt.gettype(n) ~= nil)
end
local t = rt.newtask()
print("Range is", rt.isa(rt.range(1, 2, false), rt.gettype("Range")), "Task is", rt.isa(t, rt.gettype("Task")))
rt.resolve(t, nil)

local A, B = newproxy(true), newproxy(true)
local mtA, mtB = getmetatable(A), getmetatable(B)
try("settype", rt.settype, mtA, "Sprite")
try("settype again", rt.settype, mtA, "Sprite")
try("second metatable", rt.settype, mtB, "Sprite")
try("built-in name", rt.settype, mtB, "Exception")
try("Lua type name", rt.settype, mtB, "Number")
try("nil metatable", rt.settype, nil, "X")
try("bad name", rt.settype, mtB, 5)
try("bad parent", rt.settype, mtB, "Kid", {})
try("child", rt.settype, mtB, "Kid", mtA)
try("cycle", rt.settype, mtA, "Sprite", mtB)
print("gettype Sprite", rt.gettype("Sprite") == mtA, "Exception", rt.gettype("Exception") == rt.Exception)
print("typeof", rt.typeof(A), rt.typeof(B), rt.isa(B, mtA), rt.isa(A, mtB))

try("new nil fields", rt.new, rt.Exception, nil)
try("new Task", rt.new, rt.Task, {})
try("new plain table", rt.new, {}, {})
try("exception unknown", rt.exception, "NoSuchType", "m")
try("exception not one", rt.exception, "Sprite", "m")
print("exception", rt.typeof(rt.exception("TypeError", "m")), rt.exception(nil, "x").message)
