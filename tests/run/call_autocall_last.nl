# x.member as the last argument or returned value: a method is called and all its values are kept, any other value is itself
struct Pair
  a: Number
  b: Number
end

fn Pair.sum
  @a + @b
end

fn Pair.both
  return @a, @b
end

fn show(*args)
  let out = []
  for v in args
    Table.insert(out, typeof(v) == "Function" and "fn" or tostring(v))
  end
  Table.concat(out, " ")
end

let p: any = Pair { a: 1, b: 2 }
print show(0, p.a)
print show(0, p.sum)
print show(0, p.both)
print show(p.both, 9)
print show(0, p.sum!)

let t: any = {n: 4, yes: true, no: false, f: fn(x) (x or 0) + 1 end}
print show(0, t.n)
print show(0, t.yes, t.no)
print show(0, t.f)
print show(0, t.f(1))
print show(0, t.f!)

fn last_of(o)
  return 0, o.both
end

fn field_of(o)
  return o.f
end

fn value_of(o)
  return o.n
end

print show(last_of(p))
print typeof(field_of(t)), field_of(t)(5), value_of(t)

let s: any = "héllo"
let n: any = 2.5
print show(0, s.length), show(0, n.floor)

fn second(_first, value)
  value
end

let total = 0
for i in 1..200
  let o: any = i % 2 == 0 and p or t
  total += second(0, o.n) or second(0, o.sum)
end
print total
