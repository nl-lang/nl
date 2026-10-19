# Bitwise operator definitions, including unary ~
struct Flags
  bits: Number
end

fn Flags.|(right)
  Flags { bits: @bits | right.bits }
end

fn Flags.&(right)
  Flags { bits: @bits & right.bits }
end

fn Flags.^(right)
  Flags { bits: @bits ^ right.bits }
end

fn Flags.<<(right)
  Flags { bits: @bits << right }
end

fn Flags.>>(right)
  Flags { bits: @bits >> right }
end

fn Flags.>>>(right)
  Flags { bits: @bits >>> right }
end

fn Flags.~
  Flags { bits: ~@bits }
end

let a = Flags { bits: 12 }
let b = Flags { bits: 10 }
print((a | b).bits)
print((a & b).bits)
print((a ^ b).bits)
print((a << 2).bits)
print((a >> 2).bits)
let neg = Flags { bits: -16 }
print((neg >>> 28).bits)
print((~a).bits)
let c = a
c |= b
print c.bits
