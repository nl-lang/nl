# `|` lowest, prefix `~` next, postfix `?` tightest;
# parentheses group.
let a: A | B | C
let b: ~A | B
let c: ~A?
let d: (Number | String)?
let e: ~(A | B)
