# *args collects the remaining arguments into a table, f(*list) spreads one
fn count(*args)
  args.len
end

print count()
print count(1, 2, 3)

fn first(*args)
  args[1]
end

print first("x", "y")

fn collect(*args)
  args.len
end

print collect(4, 5, 6, 7)

fn sum3(a, b, c)
  a + b + c
end

let list = [1, 2, 3]
print sum3(*list)
let two = [20, 30]
print sum3(10, *two)

fn join(*parts)
  Table.concat(parts, "-")
end

print join("a", "b", "c")

# parameters before *args take the first arguments
fn rest_len(a, b, *rest)
  rest.len
end

print rest_len(1, 2)
print rest_len(1, 2, 3, 4)

# a block needs a Function parameter; it is the final argument: here the second one, so it is callback
fn apply(a: Number, callback: Function, *args: any)
  callback(a) + args.len
end

let r = apply 1 do |x|
  x * 10
end
print r
print apply(2, fn(x) x + 1 end, "extra", "more")
