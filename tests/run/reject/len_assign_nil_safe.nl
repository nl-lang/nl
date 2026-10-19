# Rejected by the compiler: a nil-safe target is still a target.
let t = [1, 2]
# expect-error: `x?.len = v` is an error, as `x.len = v` is; `len` is reserved
t?.len = 3
