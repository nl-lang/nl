# The trace of a caught exception starts at the frame that
# raised, not with the message handler's own frames (the `debug` stand-in
# of `nlc run` honours its level argument). The C frame
# of `error` is left out too: the first frame is the NL one.
fn fail()
  raise "boom"
end
try
  fail()
catch e
  let lines = e.trace.split("\n")
  print lines[1]
  print lines[2].strip
end
include("lib/traceback.lua")
