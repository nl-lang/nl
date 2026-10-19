# Field access, receiver without a declared type: particles as struct
# instances, updated through `p.x`, `p.vx`, ... (each read is a member read
# that may auto-call: TGETS + ISMETH on the fork).
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 1000
const steps = 20000

struct Particle
  x: Number
  y: Number
  vx: Number
  vy: Number
end

let check = 0
for _rep in 1..scale
  let ps = []
  for i in 1..n
    ps[i] = Particle { x: i, y: i * 0.5, vx: (i % 7) - 3, vy: (i % 5) - 2 }
  end
  for _s in 1..steps
    for i in 1..n
      let p = ps[i]
      p.x = p.x + p.vx * 0.01
      p.y = p.y + p.vy * 0.01
      if p.x < 0 or p.x > 1000
        p.vx = -p.vx
      end
      if p.y < 0 or p.y > 1000
        p.vy = -p.vy
      end
    end
  end
  for i in 1..n
    check += ps[i].x + ps[i].y
  end
end
print Number.floor(check)
