# Method calls on structs, receivers with a declared struct type: the
# compiler resolves `v.norm2`, `acc.add(w)` and `acc.dot(v)` statically
# into plain calls (no ISMETH or MSELF).
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 6000000

struct Vec
  x: Number
  y: Number
end

fn Vec.norm2
  @x * @x + @y * @y
end

fn Vec.add(o: Vec)
  @x += o.x
  @y += o.y
end

fn Vec.dot(o: Vec): Number
  @x * o.x + @y * o.y
end

let check = 0
let xy = 0
for _rep in 1..scale
  let acc: Vec = Vec { x: 0, y: 0 }
  let v: Vec = Vec { x: 1, y: 2 }
  let w: Vec = Vec { x: 0.5, y: -0.25 }
  for _i in 1..n
    acc.add(w)
    check += acc.dot(v) * 0.001 + v.norm2
    check = check % 1000003
  end
  xy += acc.x + acc.y
end
print Number.floor(check * 1000), xy
