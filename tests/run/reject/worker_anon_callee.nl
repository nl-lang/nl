# Rejected by the resolver.
let f = fn(x) x end
# expect-error: the callee of `spawn` must be a named function; `f` is an anonymous function
let t = spawn f(1)
