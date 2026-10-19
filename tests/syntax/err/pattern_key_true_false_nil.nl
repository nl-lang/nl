# `true`, `false` and `nil` are not keys of a keyed
# brace pattern; the key is written as a string.
let {true: x, a, nil: {b}} = t
let {"true": y, "false": z} = t
