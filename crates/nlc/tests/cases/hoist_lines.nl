# Finding 4 and the type pools: definitions run before the rest of their
# block, structs before functions, which is not their source order here;
# they keep their own lines in runtime errors.
fn here(p: Pt?)
  error("here")
  p
end
print "start"
struct Pt
  x: Number?
end
try
  here(nil)
catch e
  print e.message
end
