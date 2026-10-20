# M.f! of a module is bound to M: Lua code calls it with the arguments alone
module Greeter
  const greeting = "hello"

  fn greet(name)
    "#{Self.greeting}, #{name}"
  end

  fn first(a, b)
    a
  end
end

let greet = Greeter.greet!
print greet("ann")
print :pcall(greet, "bob")
print :pcall(Greeter.first!, 1, 2)
print Greeter.greet! == Greeter.greet!
let names = ["cy", "di"]
print names.map(Greeter.greet!).join("; ")

# The raw method, which takes the module first, is the table's own value.
let raw = Greeter["greet"]
print raw(Greeter, "eve")

# A struct's method is bound to its value the same way.
struct Counter
  n: Number = 0
end

fn Counter.add(k: Number)
  @n += k
end

let c = Counter {}
let add = c.add!
add(2)
:pcall(add, 3)
print c.n

# What is not a method is read as it is: a field, a plain function, and a
# library's function read from a string, which takes the string first.
let box = {f: fn(x) x * 10 end}
let f = box.f!
print f(4)
let up = "abc".upcase!
print up("xyz")
