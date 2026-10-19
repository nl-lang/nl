# Struct patterns with shorthand and keyed items, a qualified
# name, `Self`.
let Circle {r} = c
let Rect {w, h: height} = r
let Shapes.Circle {r} = c
let Self {x} = s
