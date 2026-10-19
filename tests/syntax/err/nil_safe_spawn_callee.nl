# A `spawn` callee cannot be nil-safe; one error, and the chain is read.
let t = spawn none?.m(f())
let u = spawn a.b?.(1)
let v = spawn ok(1)
