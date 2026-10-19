# Redefining a primitive method that came from a different file is a runtime error
fn Number.twice
  self * 2
end

print 4.twice
try
  include("include/redefine_twice.nl")
  print "not reached"
catch
  print "redefinition from another file rejected"
end
print 4.twice
