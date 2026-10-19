# A `>>` or `>>>` closing type arguments is split; so is
# `>=` in `Box<T>= y`.
let a: Box<Box<T>>
let b: Box<Box<Box<T>>>
let c: Box<T>= y
