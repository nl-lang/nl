# checked-only
# `{key: nil}` requires the key to be nil and binds nothing, in a may-match
# and in a must-match context.
let v: any = {a: nil, b: 1}
if let {a: nil, b} = v
  print "nil a, b #{b}"
end
if let {b: nil} = v
  print "not reached"
else
  print "b is not nil"
end
let {a: nil} = v
print "must-match ok"
try
  let {b: nil} = v
catch err
  print typeof(err)
end
