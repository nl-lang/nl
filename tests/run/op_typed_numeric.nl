# `//` and the bitwise operators on operands declared `Number` (direct code) agree with the runtime path
fn ops(a: Number, b: Number)
  return [a // b, a & b, a | b, a ^ b, a << b, a >> b, a >>> b]
end
let r = ops(-7, 2)
print r[1], r[2], r[3], r[4], r[5], r[6], r[7]
r = ops(0xdeadbeef, 33)
print r[1], r[2], r[3], r[4], r[5], r[6], r[7]

# Never a negative zero, also inside a larger expression.
let z: Number = 0
let m: Number = -5
print z // m, z // 3, 2 * (z // m), 1 - z // m, (z // m) // 1
let small: Number = -1
print small // 1e300, 1 // 0, small // 0, 7.5 // 2
print(-0.0 // 5, 0 // -5, -0.0 // -5)

# Compound assignment.
let q: Number = 5
q //= 2
q <<= 3
q |= 1
q ^= 0xff
q &= 0x7f
q >>= 1
q >>>= 1
print q

# A loop with more bitwise operators than the JIT unrolls tail calls.
let x: Number = 2463534242
let acc: Number = 0
for i in 1..3000
  x = x ^ (x << 13)
  x = x ^ (x >>> 17)
  x = x ^ (x << 5)
  let v: Number = x & 0xffff
  v = v - ((v >>> 1) & 0x5555)
  v = (v & 0x3333) + ((v >>> 2) & 0x3333)
  v = (v + (v >>> 4)) & 0x0f0f
  acc = (acc + ((v + (v >>> 8)) & 0x1f) + i // 7) | 0
end
print x, acc
