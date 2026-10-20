# returning a call of a function declared to return a fitting type: the callee's check holds for both
fn half(x: Number): Number
  x / 2
end

fn quarter(x: Number): Number
  half(half(x))
end

fn either(x: Number): Number | String
  return half(x)
end

fn two(): Number
  return 1, 2
end

fn both(): Number
  two()
end

fn loose(v)
  v
end

fn strict(v: any): Number
  loose(v)
end

fn bad(v: any): Number
  v
end

fn outer(v: any): Number
  bad(v)
end

fn any_x(v: any): {x: any}
  v
end

fn number_x(v: any): {x: Number}
  any_x(v)
end

fn attempt(f: Function)
  try
    print f()
  catch e: TypeError
    print e.message
  end
end

print quarter(8), either(3)
print both()
let s: any = "text"
attempt(|| -> strict(s))
attempt(|| -> outer(s))
attempt(|| -> number_x({x: s}).x)
print number_x({x: 4}).x
