# A word followed by `:` at the start of an item is a key with any
# spacing, in tables, struct literals, patterns and record types.
t = {a: b, f :x, type: 1}
s = Node {type :1}
let {a :v} = t
let r: {type :String} = t
u = {f: :x}
