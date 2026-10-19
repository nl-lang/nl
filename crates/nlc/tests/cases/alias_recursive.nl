# Finding 1: a record alias may refer to itself through a field, in both
# build modes; alias expansion is linear (each alias is expanded once).
type Node = {value: Number, next: Node?}
let n: Node = {value: 1, next: {value: 2, next: {value: 3}}}
print n.next.next.value
print n is Node, 5 is Node

# Two references to itself in one record: 2^33 expansions before the fix.
type T = {a: T?, b: T?}
let t: T = {a: {}, b: {a: {}}}
print t is T

# A chain of aliases, each naming the previous one twice.
type A0 = {x: Number}
type A1 = {l: A0, r: A0}
type A2 = {l: A1, r: A1}
type A3 = {l: A2, r: A2}
type A4 = {l: A3, r: A3}
type A5 = {l: A4, r: A4}
type A6 = {l: A5, r: A5}
type A7 = {l: A6, r: A6}
type A8 = {l: A7, r: A7}
type A9 = {l: A8, r: A8}
type A10 = {l: A9, r: A9}
type A11 = {l: A10, r: A10}
type A12 = {l: A11, r: A11}
type A13 = {l: A12, r: A12}
type A14 = {l: A13, r: A13}
type A15 = {l: A14, r: A14}
type A16 = {l: A15, r: A15}
type A17 = {l: A16, r: A16}
type A18 = {l: A17, r: A17}
type A19 = {l: A18, r: A18}
type A20 = {l: A19, r: A19}
fn deep(x: A20?)
  x == nil
end
print deep(nil)

# Mutually recursive aliases through records.
type Tree = {kids: Forest?}
type Forest = {first: Tree, rest: Forest?}
let tree: Tree = {kids: {first: {}, rest: {first: {kids: nil}}}}
print tree is Tree
