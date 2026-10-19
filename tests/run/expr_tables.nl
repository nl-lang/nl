# Table constructors and access
let list = [1, 2, 3]
print list[1]
let map = { a: 1, b: 2, 1: 3 }
print map.a
print map["b"]
print map[1]
let key_expr = "k" <> "ey"
let more = { "any string": 1, }
more[key_expr] = 2
print more["any string"]
print more.key
let nested = { inner: { deep: "yes" }, list: [10, 20] }
print nested.inner.deep
print nested.list[2]
nested.added = 4
nested["other key"] = 5
print nested.added + nested["other key"]
let x = 9
let shorthand = {x: x}
print shorthand.x
let empty = {}
print empty.missing
