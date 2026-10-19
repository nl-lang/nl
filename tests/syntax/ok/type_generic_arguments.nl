# `<` after a qualified name opens type arguments; a trailing
# comma is allowed.
let a: Box<T>
let b: Map<String, Number,>
let c: Lib.Box<T>
