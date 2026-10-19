# Method calls on structs, receiver without a declared type: `v.norm2`
# (a member read that auto-calls, TGETS + ISMETH + CALL), `acc.add(v)`
# and `v.scaled(k)` (member calls, MSELF on the fork).
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 6000000

struct Vec
  x: Number
  y: Number
end

fn Vec.norm2
  @x * @x + @y * @y
end

fn Vec.add(o)
  @x += o.x
  @y += o.y
end

fn Vec.dot(o)
  @x * o.x + @y * o.y
end

let check = 0
let xy = 0
for _rep in 1..scale
  let acc = Vec { x: 0, y: 0 }
  let v = Vec { x: 1, y: 2 }
  let w = Vec { x: 0.5, y: -0.25 }
  for _i in 1..n
    acc.add(w)
    check += acc.dot(v) * 0.001 + v.norm2
    check = check % 1000003
  end
  xy += acc.x + acc.y
end
print Number.floor(check * 1000), xy
