# After `key:`, a lowercase name binds; a capitalized name,
# `nil`, `any`, `~` or `(` starts a type; a capitalized path with `{` is a
# struct pattern; `key: name: T` binds with a type.
let {a: Number} = t
let {a: Number | String} = t
let {pos: Geo.Point?} = t
let {b: Lib.Box<T>} = t
let {n: nil} = t
let {z: any} = t
let {w: ~Number} = t
let {p: (A | B)} = t
let {shape: Circle {r}} = t
let {key: value: Number} = t
let {key: value} = t
let {s: "admin"} = t
