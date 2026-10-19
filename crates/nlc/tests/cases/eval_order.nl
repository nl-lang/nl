# Finding 3: operands and targets are evaluated left to right, each once,
# whether or not a later operand needs statements.
let t = [10, 20]
let i = 1
fn bump
  i = 2
  5
end
t[i] += bump()
print t[1], t[2]

let u = [10, 20]
i = 1
u[i] = bump()
print u[1], u[2]

let o = {x: 1}
let p = {x: 100}
fn swap
  o = p
  1
end
let old = o
o.x += swap()
print old.x, o.x, p.x

let a = 1
fn set_a
  a = 100
  1
end
print a + set_a()
a = 1
print a + (if true
  set_a()
end)
a = 1
a += set_a()
print a

let w = [0, 0, 0]
i = 1
w[i], w[3] = bump(), 7
print w[1], w[2], w[3]

let calls = 0
fn key
  calls += 1
  1
end
let k = [5]
k[key()] += 10
print k[1], calls

let j = 1
let v = []
v[j] = (if true
  j = 2
  "x"
end)
print v[1], v[2]
