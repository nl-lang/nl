# `true`, `false` and `nil` before `:` are not
# table keys; one error per item, the value is read, and the other items and
# the next statement are unaffected.
t = {true: 1, a: 2, false: f(3), nil: "x", [true]: 4, ["nil"]: 5}
u = 1
