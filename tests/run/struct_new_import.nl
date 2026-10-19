# A literal of a struct from another file, by its name, through the export table and through an alias, run many times
import { Vec, Monster, noted } from 'import/shapes'
import from 'import/shapes' as Shapes
type V = Vec

let total = 0
for i in 1..300
  let a = Vec { x: i, y: 1 }
  let b = Shapes.Vec { y: i, x: 2 }
  let c = V { x: a.sum, y: b.sum }
  total += c.sum
end
print total

let v = Shapes.Vec { x: 1, y: 2 }
print typeof(v), v is Vec, v is V
let t = Shapes.Tagged { y: 2, x: 1 }
print typeof(t), t is Vec, t.name, t.sum

# Omitted fields: the defaults, and nil for an optional field.
let seen = 0
for i in 1..300
  let m = Monster { id: i, pos: Vec { x: i, y: i } }
  seen += m.level + m.pos.x
  seen += 1 if m.boss == nil and m.loot == "none" and m.name == "unnamed"
end
print seen
print noted().split(" ").len

let m = Shapes.Monster { loot: "gold", level: 3, boss: true, name: "bat", id: 7 }
print m.describe, typeof(m), m is Shapes.Entity, m.pos
print noted() == ""
