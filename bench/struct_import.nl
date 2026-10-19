# Construction of structs declared in another file: `Vec { ... }` by an
# imported name and `Lib.Body { ... }` through the export table (a parent's
# field, a default, an optional field left out) per iteration, and a
# parameter of the imported type, against setmetatable on a table
# constructor in Lua.
import { Vec, Body } from 'struct_import_lib'
import from 'struct_import_lib' as Lib
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 4000000

fn weigh(b: Body)
  b.mass * b.pos.x
end

let check = 0
for _rep in 1..scale
  let prev = Vec { x: 0, y: 0 }
  for i in 1..n
    let v = Vec { x: prev.y + 1, y: i % 13 }
    let b = Lib.Body { pos: v, id: i }
    check += weigh(b) + v.y
    prev = v
  end
  check = check % 1000003
end
print check
