# The fold rule of a trace (runtime/nl_50_error.lua)
# on the chunk names of the runtime and the embedder, in both spellings
# (`nlc run` and the fork), and on a Lua file named nl_*.lua, which stays.
let chain = include("lib/trace_fold.lua")
let call = chain(fn() raise "folded" end)
try
  call()
catch e
  print e.trace
end
