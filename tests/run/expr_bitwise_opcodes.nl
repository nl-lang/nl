# Bitwise operators on operands declared `Number` (the VM's bit opcodes on the fork) agree with the `bit` library at the edges
fn ops(name: String, a: Number, b: Number)
  print name, a & b, a | b, a ^ b, a << b, a >> b, a >>> b, ~a
end

# A number that is not an integer value in a dual-number VM (ARM64): the
# same value as `x`, computed in floating point.
fn float(x: Number): Number
  x * 0.5 * 2
end

let nan: Number = 0 / 0
let inf: Number = 1 / 0
let nz: Number = 0 * -1

ops("ints", 5, 3)
ops("negative", -7, 2)
ops("float ints", float(5), float(3))
ops("fractions", 3.7, 1.5)
ops("negative fractions", -3.7, -0.5)
ops("halves", 2.5, 3.5)
ops("2^31", 2147483648, 1)
ops("2^31+1", 2147483649, 31)
ops("-2^31", -2147483648, 1)
ops("-2^31-1", -2147483649, 1)
ops("2^32", 4294967296, 32)
ops("2^32+5", 4294967301, 33)
ops("2^53", 9007199254740992, 3)
ops("1e10", 1e10, 1e10)
ops("-0", nz, nz)
ops("nan", nan, 1)
ops("inf", inf, 1)
ops("-inf", -inf, 1)

# Constant operands, which the compiler may fold.
print 3.7 & 5, 2.5 | 0, -3.7 ^ 0, 4294967301 << 1, 2147483648 >> 1, 1e10 >>> 33, 1 << 32.5
print ~2.5, ~3.5, ~-0.0, ~2147483648, ~-2147483649

# Shift counts are taken modulo 32 after the conversion to 32 bits.
fn shifts(count: String, a: Number, b: Number)
  print count, a << b, a >> b, a >>> b
end
let counts: [Number] = [0, 31, 32, 33, -1, 1e10, 1.5, -31.5, 4294967297, 2147483648]
for c in counts
  shifts(c as String, -5, c)
  shifts(c as String, float(-5), float(c))
end
shifts("nan", -5, nan)
shifts("inf", -5, inf)
shifts("-inf", -5, -inf)

# Compound assignment on a declared variable.
let q: Number = 0xdeadbeef
q &= 0xfff0fff0
q |= 3.9
q ^= -1
q <<= 33
q >>= 2.5
q >>>= 1e10 + 1
print q

# A loop the JIT compiles, over integers, fractions, large values and the
# special ones, with every operator.
let vals: [Number] = [7, -9, 3.7, -0.5, 2147483648, 4294967301, -2147483649, 1e10, nz, nan, inf, float(12)]
let acc: Number = 0
for i in 1..300
  for j in 1..vals.len
    let a: Number = vals[j]
    let b: Number = vals[(i + j) % vals.len + 1]
    acc = acc ^ (a & b) ^ ((a | b) << 1) ^ ((a ^ b) >>> 3) ^ (a << (b & 7)) ^ (a >> b) ^ ~(a >>> i) ^ i
  end
end
print acc
