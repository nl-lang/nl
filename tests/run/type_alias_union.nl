# Aliases, unions, optional, negated and function types
type Id = Number | String
type NotNumber = ~Number

fn show(id: Id): String
  "id #{id}"
end

print show(1)
print show("a")
let flag: any = true
try
  show(flag)
catch err: TypeError
  print "Bool is not an Id"
end

fn maybe(n: Number?): String
  if n == nil
    return "nothing"
  end
  "got #{n}"
end

print maybe(nil)
print maybe(2)

fn not_num(v: NotNumber)
  typeof(v)
end

print not_num("s")
let num: any = 3
try
  not_num(num)
catch err: TypeError
  print "Number rejected"
end

fn apply(f: Function(a: Number): Number, x: Number): Number
  f(x)
end

print apply(fn(a) a + 1 end, 1)
let counts: {[String]: Number} = {a: 1}
print counts.a
