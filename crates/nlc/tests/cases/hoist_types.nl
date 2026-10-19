# Finding 4: what the resolver hoists (fn, struct, enum, interface) is
# defined before any other statement of its block runs.
struct B : A
  y: Number = 2
end
struct A
  x: Number = 1
end
let b = B {}
print b is A, b.x, b.y

fn mk
  C { z: 3 }
end
let c = mk()
print c.z
struct C
  z: Number
end

fn P.hello
  "hi"
end
struct P
  x: Number = 1
end
let pv = P {}
print pv.hello

print first_color(), Color.values.len
enum Color
  Red
  Green
end
fn first_color
  Color.Red
end

fn local_types
  let d = D { n: 4 }
  struct D
    n: Number
  end
  d.n
end
print local_types()
