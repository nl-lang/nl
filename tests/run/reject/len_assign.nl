# Rejected by the compiler.
let t = [1, 2]
# expect-error: `x.len = v` is an error; `len` is reserved (use `t["len"]` for a key of that name)
t.len = 3
