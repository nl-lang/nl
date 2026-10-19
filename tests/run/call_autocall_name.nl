# A bare name is called when it is function-kind or discarded
fn five
  5
end

let six = fn 6 end
let v = five
print v
print six
print typeof(five)
print typeof(five!)
let f = five!
print typeof(f)
print f()
let g = f
print typeof(g)

fn call_typed(cb: Function)
  cb
end

print call_typed(six!)

fn call_maybe(cb: Function?)
  if cb
    return cb()
  end
  "none"
end

print call_maybe(six!)
print call_maybe(nil)

let count = 0
fn tick
  count += 1
end

let h = tick!
h
tick
print count

fn kind_of(x)
  typeof(x)
end

print kind_of(five)
print kind_of(five!)
