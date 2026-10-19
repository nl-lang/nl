# `|a, b| -> expr` and `|| -> expr`; the parameters
# are block parameters, a trailing comma and `| |` included.
let mul = |a, b| -> a * b
let yes = || -> true
let none = | | -> nil
let first = |a, b,| -> a
