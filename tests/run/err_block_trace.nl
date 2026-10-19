# Errors raised inside a block that is made once, and inside the function that calls it, keep their frames, lines and checks
fn each(list: any, f: Function)
  for v in list
    f(v)
  end
end

fn checked(x: Number, f: Function): Number
  f(x)
end

fn failing(list: any, f: Function)
  for v in list
    raise "helper failed at #{v}" if v == 2
    f(v)
  end
end

let xs = [1, 2, 3]

# An error inside the block.
try
  for i in 1..2
    each xs do |v|
      raise "block failed at #{v * i}" if v == 2
    end
  end
catch e
  print e.message
  print e.trace
end

# An error inside the helper.
try
  for i in 1..2
    failing xs do |v|
      print v + i
    end
  end
catch e
  print e.message
  print e.trace
end

# A Lua error inside the block names the captured variable.
try
  for i in 1..2
    let t: any = nil
    each xs do |v|
      print t.field + i
    end
  end
catch e
  print typeof(e)
  print e.trace
end

# The checks of the helper's parameters and of the block's typed
# parameter and return run as before.
let bad: any = "x"
try
  for i in 1..2
    checked(bad) do |v| v + i end
  end
catch e: TypeError
  print e.message
end
try
  for i in 1..2
    checked(i) do |v: String| v end
  end
catch e: TypeError
  print e.message
end
try
  for i in 1..2
    checked(i) do |v|: String v end
  end
catch e: TypeError
  print e.message
end
try
  for i in 1..2
    checked(i) do |v| "s" end
  end
catch e: TypeError
  print e.message
end
