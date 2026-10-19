# A value of type `Function(): Number` may be a
# Lua function returning anything (a signature is checked as "a
# function" only), so its result is no proven Number and `//` and `&` keep
# the runtime path, which refuses a string, with or without static resolution.
# backends: lua,bc
let f: Function(): Number = include("lib/returns_string.lua")
try
  print(f() // 3)
catch e
  print "// raised"
end
try
  print(f() & 3)
catch e
  print "& raised"
end
