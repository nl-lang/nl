# Rejected by the compiler.
let t = [1, 2]
# expect-error: `x.len!` is an error; `len` is not looked up in the value
let f = t.len!
