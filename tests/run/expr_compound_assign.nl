# Compound assignment for every operator; the target is evaluated once
let a = 10
a += 5
print a
a -= 3
print a
a *= 2
print a
a /= 8
print a
a //= 2
print a
a = 7
a %= 4
print a
a **= 2
print a
let b = 12
b |= 1
print b
b &= 6
print b
b ^= 7
print b
b <<= 3
print b
b >>= 2
print b
let c = -8
c >>>= 28
print c
let s = "ab"
s <>= "cd"
print s
let v = nil
v ||= 3
print v
v ||= 4
print v
let w = 1
w &&= 9
print w
let z = false
z &&= 9
print z

let idx_calls = 0
let t = [5]
fn idx
  idx_calls += 1
  1
end

t[idx()] += 10
print t[1]
print idx_calls
