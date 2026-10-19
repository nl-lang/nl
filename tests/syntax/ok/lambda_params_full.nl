# The parameters of a short function are block
# parameters: annotations, a return type, defaults, patterns, `*rest` and
# `**opts`.
let area = |w: Number, h: Number|: Number -> w * h
let greet = |name, greeting = "hello"| -> greeting <> name
let fst = |{x, y}| -> x
let count = |first, *rest| -> rest.len
let opt = |x, **o| -> o.unit
