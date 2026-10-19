# Struct construction: `Vec { x: ..., y: ... }` per iteration (a struct
# with typed fields, one with a default), against setmetatable on a table
# constructor in Lua.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 6000000

struct Vec
  x: Number
  y: Number
  w: Number = 1
end

let check = 0
for _rep in 1..scale
  let prev = Vec { x: 0, y: 0 }
  for i in 1..n
    let v = Vec { x: prev.y + 1, y: i % 13 }
    check += v.x + v.y * v.w
    prev = v
  end
  check = check % 1000003
end
print check
