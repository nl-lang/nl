# The closures of `try` are made once and passed what they read
# (crates/nl_ir/src/lower/lift.rs); what they share with the code
# around them must behave as if they were made where the `try` stands.
# backends: lua,bc
struct Odd : Exception
  n: Number
end

fn check(n)
  raise Odd { message: "odd", n: n } if n % 2 == 1
  n
end

# A loop variable read in the body, a file-level local assigned in a
# catch clause, a type test.
let odd = 0
let sum = 0
for i in 1..10
  let v = try
    check(i)
  catch e: Odd
    odd += e.n
    0
  end
  sum += v
end
print sum, odd

# A local the body assigns, and one a function called by the body
# assigns: both are seen where they are changed.
fn counter
  let count = 0
  let bump = fn() count += 1 end
  let seen = []
  for _ in 1..3
    try
      bump()
      Table.insert(seen, count)
    catch
      print "unused"
    end
  end
  let last = nil
  try
    last = count * 10
  catch
    print "unused"
  end
  print Table.concat(seen, ","), last
end
counter()

# A function made in the body sees later assignments of what it uses.
fn later
  let x = 1
  let get = nil
  try
    get = fn() x end
  catch
    print "unused"
  end
  x = 2
  print get()
end
later()

# A function made in the body, from a loop variable (never assigned).
fn makers
  let fs = []
  for i in 1..3
    try
      Table.insert(fs, fn() i * 100 end)
    catch
      print "unused"
    end
  end
  print fs[1](), fs[2](), fs[3]()
end
makers()

# Parameters, `return`, `break` and `continue` in the body, and a
# `finally` that overrides the outcome.
fn find(list, target)
  let steps = 0
  for x in list
    try
      steps += 1
      continue if x < 0
      return "found #{x} after #{steps}" if x == target
      break if x > 100
    finally
      steps += 0
    end
  end
  "not found after #{steps}"
end
print find([1, -2, 3], 3)
print find([1, 200, 3], 3)

fn override(n)
  try
    return n
  finally
    return n + 1 if n > 5
  end
end
print override(1), override(10)

# Nested `try`: the inner one reads a variable of the outer body and a
# parameter; the outer catch reads the error of the inner one.
fn nested(a)
  try
    let b = a * 2
    let r = try
      check(b + 1)
    catch e: Odd
      raise "inner caught #{e.n} with #{a}"
    end
    r
  catch e
    e.message
  end
end
print nested(4)

# A `try` that is a function's tail and returns every value of a call,
# reading a parameter: the packed form.
fn pair(x)
  return x, x + 1
end
fn tail_pair(x)
  try
    pair(x)
  catch
    0
  end
end
print tail_pair(5)

# A `try` in a closure made per iteration, reading the loop variable
# and assigning a local of the enclosing function.
fn per_iteration
  let total = 0
  let fs = []
  for i in 1..3
    Table.insert(fs, fn()
      try
        total += check(i * 2)
      catch e
        total = -1
      end
    end)
  end
  for f in fs
    f()
  end
  print total
end
per_iteration()

# An error that no clause catches goes on, after `finally`.
fn uncaught
  try
    try
      raise "deep"
    catch e: Odd
      print "wrong clause"
    finally
      print "inner finally"
    end
  catch e
    print "outer: #{e.message}"
  end
end
uncaught()
