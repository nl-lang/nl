# a function without **opts refuses keyed arguments in whichever parameter they land
fn three(a, b, c)
  "three #{a} #{b} #{c}"
end

fn typed(x: Number, t: Table, y: any)
  "typed #{x} #{y}"
end

fn defaults(a, b = 2)
  "defaults #{a} #{b}"
end

struct Point
  x: Number
end

fn Point.move(dx, dy)
  "move #{@x + dx} #{dy}"
end

fn attempt(label: String, f: Function)
  try
    print f()
  catch e: TypeError
    print "#{label}: #{e.message}"
  end
end

let three_any: any = three!
let typed_any: any = typed!
let defaults_any: any = defaults!
let p: any = Point { x: 1 }
attempt "first", || -> three_any(k: 1)
attempt "second", || -> three_any(1, k: 1)
attempt "third", || -> three_any(1, 2, k: 1)
attempt "after nil", || -> three_any(1, nil, k: 1)
attempt "spread", || -> three_any(*[1, 2], **{k: 1})
attempt "number", || -> typed_any(k: 1)
attempt "table", || -> typed_any(1, k: 1)
attempt "last typed", || -> typed_any(1, {}, k: 1)
attempt "default", || -> defaults_any(k: 1)
attempt "default given", || -> defaults_any(1, k: 1)
attempt "method", || -> p.move(k: 1)
attempt "method second", || -> p.move(1, k: 1)
print three(1, 2, 3)
print three(1)
print typed(1, {}, 3)
print defaults(1)
print Point { x: 1 }.move(2, 3)
