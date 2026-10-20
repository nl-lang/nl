# checked-only
# A `(A, B)` return type: each value is checked, a missing value is nil and
# fails unless its type allows it. A `return` that writes its values
# returns those (`return 7` gives one value), and one whose last value is a
# call gives exactly the declared count.
fn pair(): (Number, String)
  123, 'string'
end
let a, b = pair()
print a, b
fn tail(): (Number, String)
  three()
end
fn three()
  return 1, 'two', 'three'
end
print select('#', tail())
fn short(): (Number, String?)
  return 7
end
let x, y = short()
print x, y == nil
print select('#', short())
fn bad(): (Number, String)
  return 1, 2
end
fn missing(): (Number, String)
  return 1
end
for f in [bad!, missing!]
  try
    f()
  catch err
    print typeof(err), err.message
  end
end
fn typed(cb: Function(): (Number, Number)): Number
  let p, q = cb()
  p + q
end
print typed(fn() 2, 3 end)
