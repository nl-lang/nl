# Where a newline ends a statement, and `;`
let sum = 1 +
  2
print sum
let p, q = 10,
  20
print p + q
let v =
  42
print v
let up = "abc".
  upcase
print up
let down = "XYZ"
  .downcase
print down
let total = 1
  + 2
  + 3
print total
let both = true
  && false
print both
let list = [
  1,
  2,
  3,
]
print list.len
print(1 +
  1)
let a = 1; let b = 2; print a + b

# a line starting with - ! ~ { ( or : always begins a new statement
fn negate(x)
  let y = x
  -y
end
print negate(5)

fn invert(x)
  let y = x
  !y
end
print invert(true)

fn flip_bits(x)
  let y = x
  ~y
end
print flip_bits(0)

fn pair(x)
  let y = x
  [y, y]
end
print pair(4).len

let w = 3
(fn() print("paren starts a statement") end)()
let z = 4
:print("path starts a statement")
print w + z
