# `err.trace` lists the program's
# frames with their NL lines, innermost first, and leaves out the runtime
# library and C functions. The same on both backends.
# backends: lua,bc
import { fail_in_lib } from 'lib/trace_lib'

struct Point
  x: Number
end

fn Point.shifted(d)
  raise "cannot shift by #{d}" if d < 0
  Point { x: self.x + d }
end

fn inner(n)
  raise "inner failed with #{n}"
end

fn middle(n)
  inner(n + 1)
  n
end

fn outer(n)
  middle(n * 2)
  n
end

fn each(list: any, f: Function)
  for v in list
    f(v)
  end
  nil
end

# A chain of calls.
try
  outer(1)
catch e
  print e.message
  print e.trace
end

# A method.
try
  let p = Point { x: 1 }
  p.shifted(-1)
catch e
  print e.message
  print e.trace
end

# A block, called by a function.
try
  each([1, 2, 3]) do |v|
    raise "block failed at #{v}" if v == 2
  end
catch e
  print e.message
  print e.trace
end

# A closure stored in a variable.
let check = fn(v)
  raise "closure failed" unless v
  v
end
try
  check(false)
catch e
  print e.message
  print e.trace
end

# A function of an imported file.
try
  fail_in_lib(3)
catch e
  print e.message
  print e.trace
end

# A `catch` clause that raises: its function starts on the `catch` line.
try
  try
    raise "first"
  catch e
    raise "second after #{e.message}"
  end
catch e
  print e.message
  print e.trace
end
