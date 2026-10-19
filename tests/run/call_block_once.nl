# A block passed to a function that only calls it is made once; what it captures, and what it does, is unchanged
fn each(list: any, f: Function)
  for v in list
    f(v)
  end
end

fn apply(x: any, f: Function): any
  f(x)
end

fn through(list: any, f: Function)
  each(list, f!)
end

fn wrapped(list: any, f: Function)
  each list do |v|
    f(v * 10)
  end
end

fn side(name: String, v: any): any
  print "arg #{name}"
  v
end

let xs = [1, 2, 3]

# A block capturing the loop variable sees each iteration's value.
let s1 = 0
for i in 1..3
  each xs do |v|
    s1 += v * i
  end
end
print s1

# An accumulator declared outside the loop is shared with the block.
let total = 0
for _i in 1..3
  each xs do |v|
    total += v
  end
end
print total

# An accumulator declared inside the loop starts over each time.
for i in 1..2
  let acc = 0
  each xs do |v|
    acc += v + i
  end
  print acc
end

# A local of the loop body that the body assigns before the call.
let s2 = 0
for i in 1..3
  let factor = i
  factor = factor * 2
  s2 += apply(i, fn(v) v + factor end)
end
print s2

# A variable declared before the loop and assigned in it.
let s3 = 0
let best = 0
for i in 1..3
  best = i * 10
  each xs do |v|
    s3 += v + best
  end
end
print s3

# Nested blocks: the inner block captures the outer block's parameter.
let rows = [[1, 2], [3, 4]]
let s4 = 0
for k in 1..2
  each rows do |row|
    each row do |v|
      s4 += v * k + row.len
    end
  end
end
print s4

# Through two helpers: passed on with `!`, and wrapped in another block.
let s5 = 0
for k in 1..2
  through xs do |v|
    s5 += v + k
  end
  wrapped xs do |v|
    s5 += v
  end
end
print s5

# Arguments, keyed arguments and the block run in the written order.
fn ordered(a: any, b: any, f: Function, **opts: any): any
  print "call #{opts.key}"
  f(a + b)
end
for i in 1..2
  let r = ordered side("a", i), side("b", 1), key: side("key", "k") do |v|
    print "block #{v}"
    v * 2
  end
  print r
end

# `return` in a block returns from the block; loops in a block keep their
# `break` and `continue`.
let s6 = 0
let inner = 0
for i in 1..3
  s6 += apply(i) do |v|
    for j in 1..10
      continue if j % 2 == 0
      break if j > 5
      inner += j
    end
    return v * 100 if v == 2
    v
  end
end
print s6
print inner

# Recursion: a block inside a recursive function captures its parameter.
fn depth(n: Number): Number
  return 0 if n == 0
  let sum = 0
  each xs do |v|
    sum += v * n
  end
  sum + depth(n - 1)
end
print depth(3)

# A block passed down a recursion.
fn countdown(n: Number, f: Function)
  if n > 0
    f(n)
    countdown(n - 1, f!)
  end
end
let seen = []
for i in 1..2
  countdown 2 do |n|
    Table.insert(seen, n * i)
  end
end
print Table.concat(seen, ",")

# A function that keeps or returns its argument gets a new function at
# every evaluation, as before.
let kept = []
let returned = []
fn keep(f: Function)
  Table.insert(kept, f!)
end
fn same(f: Function): Function
  f!
end
for i in 1..2
  keep do |v|
    v + i
  end
  Table.insert(returned, same(fn(v) v end))
end
print kept[1] == kept[2]
print returned[1] == returned[2]
print kept[1](1)
print kept[2](1)

# A default parameter of the callee after the block's.
fn times(f: Function, n: Number = 2)
  for i in 1..n
    f(i)
  end
end
for k in 1..2
  times do |i|
    print "#{k}:#{i}"
  end
end
