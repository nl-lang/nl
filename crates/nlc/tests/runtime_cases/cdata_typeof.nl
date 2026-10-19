# Typeof never gives nil (LuaJIT's cdata), so messages that name
# the type work.
let c = include("lib/cdata.lua")
print typeof(c)
fn want(n: Number)
  n
end
try
  want(c)
catch e: TypeError
  print e.message
end
let t: any = {}
try
  c // t
catch e
  print e.message
end
