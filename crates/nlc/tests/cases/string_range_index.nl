# `s[a..b]` clamps and gives "" when nothing is selected, so
# it is a String, never nil; `s[i]` out of range is nil (String?).
let s = "hello"
let a: String = s[1..2]
let b: String = s[4...9]
let c: String = s[7..9]
let r = 2..3
let d: String = s[r]
let e = s[9]
print(a, b, c == "", d, e == nil)
