# `let d: Number` without a value is valid; the
# type is checked on later assignments only.
let d: Number
print d
d = 5
print d
fn later
  let e: String
  e = "s"
  e
end
print later()
