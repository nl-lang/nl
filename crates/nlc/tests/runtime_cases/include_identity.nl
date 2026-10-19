# One file has one identity however its path is spelled. The
# file defines a Number method and an unnamed `on` handler and prints the
# handler name (its canonical name).
include("lib/prim.nl")
include("./lib/prim.nl")
include("lib/../lib/prim.nl")
print 4.twice
Event.run("Ping")
try
  include("lib/no_such_file.nl")
catch e
  print e.message
end
