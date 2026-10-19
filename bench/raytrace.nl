# Mixed workload: a small ray tracer. Vectors are structs with typed
# methods, spheres are structs in a list, shading uses Number.sqrt and
# Number.max; one ray per pixel, one shadow ray per hit.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const width = 480
const height = 360

struct Vec3
  x: Number
  y: Number
  z: Number
end

fn Vec3.add(o: Vec3): Vec3
  Vec3 { x: @x + o.x, y: @y + o.y, z: @z + o.z }
end

fn Vec3.sub(o: Vec3): Vec3
  Vec3 { x: @x - o.x, y: @y - o.y, z: @z - o.z }
end

fn Vec3.scale(k: Number): Vec3
  Vec3 { x: @x * k, y: @y * k, z: @z * k }
end

fn Vec3.dot(o: Vec3): Number
  @x * o.x + @y * o.y + @z * o.z
end

fn Vec3.unit: Vec3
  let len = Number.sqrt(@x * @x + @y * @y + @z * @z)
  Vec3 { x: @x / len, y: @y / len, z: @z / len }
end

struct Sphere
  center: Vec3
  radius: Number
  shade: Number
end

# The distance along the ray to the nearest hit of `s`, or nil.
fn Sphere.hit(origin: Vec3, dir: Vec3): Number?
  let oc = origin.sub(@center)
  let b = oc.dot(dir)
  let c = oc.dot(oc) - @radius * @radius
  let disc = b * b - c
  return nil if disc < 0
  let t = -b - Number.sqrt(disc)
  return nil if t < 0.001
  t
end

let spheres = [
  Sphere { center: Vec3 { x: 0, y: -1, z: 3 }, radius: 1, shade: 0.9 },
  Sphere { center: Vec3 { x: 2, y: 0, z: 4 }, radius: 1, shade: 0.7 },
  Sphere { center: Vec3 { x: -2, y: 0, z: 4 }, radius: 1, shade: 0.5 },
  Sphere { center: Vec3 { x: 0, y: -5001, z: 0 }, radius: 5000, shade: 0.3 },
]
let light: Vec3 = Vec3 { x: 1, y: 1, z: -1 }.unit

fn nearest(origin: Vec3, dir: Vec3): (Number?, Sphere?)
  let best = nil
  let best_s = nil
  for s in spheres
    let t = s.hit(origin, dir)
    if t != nil and (best == nil or t < best)
      best = t
      best_s = s
    end
  end
  return best, best_s
end

fn trace(origin: Vec3, dir: Vec3): Number
  let t, s = nearest(origin, dir)
  return 0.1 if t == nil
  let p = origin.add(dir.scale(t))
  let normal = p.sub(s.center).unit
  let lit = Number.max(0, normal.dot(light))
  let st, _ = nearest(p, light)
  lit *= 0.3 if st != nil
  s.shade * lit + 0.05
end

let check = 0
let eye = Vec3 { x: 0, y: 0, z: -1 }
for _rep in 1..scale
  for py in 0...height
    for px in 0...width
      let dir = Vec3 { x: (px - width / 2) / height, y: (height / 2 - py) / height, z: 1 }.unit
      check += Number.floor(trace(eye, dir) * 255)
    end
  end
end
print check
