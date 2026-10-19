# A Lua script can assign a `global` anything,
# so its declared type is not relied on and `//` and `&` keep
# the runtime path, which refuses a string, with or without static resolution.
# backends: lua,bc
global G: Number = 1
include("lib/set_global_string.lua")
try
  print(G // 3)
catch e
  print "// raised"
end
try
  print(G & 3)
catch e
  print "& raised"
end
