# `Function` with a signature whose `(` touches it; named,
# bare and rest parameters; a return type.
let a: Function
let b: Function(a: Number, b: String): nil
let c: Function(Number, String)
let d: Function(Number, *rest: Number): Bool
let e: Function(): Function(): nil
