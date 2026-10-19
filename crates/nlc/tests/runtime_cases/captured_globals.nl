# The runtime takes what it needs of the Lua environment
# when it opens; a script that replaces globals and library fields
# afterwards does not break it.
include("lib/vandal.lua")
let s = "héllo"
let spaced = ""
for c in s
  spaced = spaced <> c <> " "
end
print spaced
print s[2], s.reverse, s.length
let f = load_nl("print 'load_nl ok'", "chunk")
f()
include("lib/hello.nl")
let mod = require("lib.module")
print mod.answer
let x: Number | String = 1
let table_value: any = {}
try
  let y: (Number | String)? = table_value
catch e: TypeError
  print e.message
end
try
  raise "boom"
catch e
  print typeof(e), e.trace != nil
end
async fn compute()
  41 + 1
end
compute().on_done(fn(v) print v end)
Event.on_run("E", "h", fn() print "handler" end)
Event.remove("E", "h")
Event.run("E")
async fn failing()
  raise "unobserved"
end
failing()
print "done"
