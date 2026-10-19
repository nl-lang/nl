# Short functions `x -> e`, `|a, b| -> e`, `|| -> e` are `fn(params) e end`
fn each(list: any, f: Function)
  for v in list
    f(v)
  end
end

fn apply(x: any, f: Function): any
  f(x)
end

fn map(list: any, f: Function): any
  let out = []
  for v in list
    out[out.len + 1] = f(v)
  end
  out
end

fn reduce(list: any, init: any, f: Function): any
  let acc = init
  for v in list
    acc = f(acc, v)
  end
  acc
end

let xs = [1, 2, 3, 4]

# The three spellings of the parameters.
let inc = x -> x + 1
let mul = |a, b| -> a * b
let yes = || -> true
print inc(1), mul(2, 3), yes()

# Typed parameters and a return type, a default, the rest and the keyed.
let area = |w: Number, h: Number|: Number -> w * h
let greet = |name, greeting = "hello"| -> "#{greeting}, #{name}"
let count = |first, *rest| -> "#{first} and #{rest.len} more"
let opt = |x, **o| -> "#{x} #{o.unit}"
print area(3, 4)
print greet("ann"), greet("bob", "hi")
print count("a", "b", "c")
print opt(5, unit: "kg")

# A block body: statements, and `return` with several values.
let minmax = |list| -> do
  let lo = list[1]
  let hi = list[1]
  for v in list
    lo = v if v < lo
    hi = v if v > hi
  end
  return lo, hi
end
let lo, hi = minmax([5, 2, 9, 4])
print lo, hi

# The body extends as far as an expression does; lambdas nest to the right.
let add = a -> b -> a + b
print add(10)(5)
print map(xs, x -> x * x).len, reduce(xs, 0, |acc, v| -> acc + v)
print reduce(map(xs, x -> x > 2), 0, |n, big| -> big and n + 1 or n)

# As a command-call argument: `f x -> e` passes the function.
let seen = []
each xs, v -> Table.insert(seen, v * 10)
print Table.concat(seen, ",")
let r = apply 7, n -> n - 1
print r

# Stored in a `let`: a function-kind name, called where it is used and
# passed with `!`.
let twice = x -> x * 2
print twice(21), apply(4, twice!)

# In a loop, passed to a function that only calls it: the same results as
# the `fn` form and as a block.
let s1 = 0
let s2 = 0
let s3 = 0
for i in 1..3
  s1 += apply(i, v -> v * 10 + i)
  s2 += apply(i, fn(v) v * 10 + i end)
  s3 += apply i do |v| v * 10 + i end
end
print s1, s2, s3

# A closure over a loop variable, kept by the callee: a new function each time.
let kept = []
for i in 1..3
  kept[i] = || -> i * 100
end
print kept[1](), kept[2](), kept[3]()

# Through a Lua path: a Lua function receives a plain function.
let t = [3, 1, 4, 1, 5]
:table.sort(t, |a, b| -> a > b)
print Table.concat(t, " ")

# In a method, `self` is the method's.
struct Scale
  factor: Number
end
fn Scale.all(list: any): any
  map(list, x -> x * @factor)
end
print Table.concat(Scale { factor: 3 }.all(xs), ",")
