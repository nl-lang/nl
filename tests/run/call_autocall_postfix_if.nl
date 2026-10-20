# x if c as the last statement follows the rule of x alone: a value is returned, a function-kind name is called
fn five
  5
end

# a parameter of unknown type is a value, as `entry` alone would be
let pick = fn(entry, ok) entry if ok end
let skip = fn(entry, ok) entry unless ok end

print typeof(pick(five!, true))
print pick(five!, true)()
print pick(five!, false) == nil
print typeof(skip(five!, false))
print skip(five!, true) == nil

# a function-kind name is called, as `five` alone would be
fn maybe_five(ok)
  five if ok
end

print maybe_five(true)
print maybe_five(false) == nil

# in a branch whose value is used
fn describe(n)
  if n > 0
    "positive" if n < 100
  else
    "not positive"
  end
end

print describe(5)
print describe(500) == nil
print describe(-1)

# several values
fn both(ok)
  1, 2 if ok
end

print both(true)
print both(false) == nil

# a statement that is not last is still discarded: a bare function-kind name is called
let count = 0
fn tick
  count += 1
end

fn run(ok)
  tick if ok
  tick unless ok
  count
end

print run(true)
print run(false)
