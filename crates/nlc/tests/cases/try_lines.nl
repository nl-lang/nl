# An error inside a `try` body names the
# line of the statement that raised it, not the line of the `catch`.
let t: any = {}
try
  print 1
  print(t + 2)
  print 3
catch e
  print e.message
end
try
  let s: any = "6"
  print(s // 2)
catch e
  print e.message
finally
  print "finally"
end
let v = try
  t.x.y
catch e
  e.message
end
print v
