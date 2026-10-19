# `A { … }` where the alias `A` does not
# name a struct is a compile error, not "cannot construct nil" at runtime.
type A = {x: Number}
# expect-error: `A` is an alias of a record, not a struct
let a = A { x: 1 }
