# An intrinsic called as a function's last
# expression is not a tail call, so the frame that applied it stays in the
# trace and its error names the line that raised (here `rt.mget` on nil);
# also in `if`/`elseif` tests and in a `when` guard, which the Lua backend
# prints on their own lines. The same on both backends.
# backends: lua,bc

fn getter(o)
  o.x.y
end

fn classify(v: any)
  if v == 0
    "zero"
  elseif v.size > 3
    "big"
  else
    "small"
  end
end

fn kind(v: any)
  case v
  when is Number
    "number"
  when "abcd" if v.nope.x
    "long string"
  else
    "other"
  end
end

fn many(o)
  o.pair
end

try
  getter({x: nil})
catch e
  print e.message
  print e.trace
end

# The text after the position differs between the runtime's member read and
# the fork's opcode (the VM's own message), so only the position is printed.
try
  classify(nil)
catch e
  print e.message[0..27]
  print e.trace
end

try
  kind("abcd")
catch e
  print e.message[0..27]
  print e.trace
end

# A getter method that returns two values still returns both.
struct Two
  a: Number
end
fn Two.pair
  return self.a, self.a + 1
end
let t = Two { a: 1 }
let p, q = many(t)
print p, q
