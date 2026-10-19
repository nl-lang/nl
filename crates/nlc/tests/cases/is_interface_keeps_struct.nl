# `p is I` narrows a struct `P` that the file does not show
# implementing `I` to `P & I`, as TypeScript does: the implementation is
# a runtime statement and may be in another file (or, as here, be that of
# a child struct). Both the struct's fields and the interface's methods
# are typed in the branch. A value of no known struct narrows to `I`.
interface Labeled
  fn label: String
end
struct P
  x: Number
end
struct Kid : P
  z: Number = 0
end
struct Q
  y: Number
end
interface Labeled for Kid
  fn label: String
    "Kid#{@x}"
  end
end

fn take(p: P)
  if p is Labeled
    let q: P = p
    let l: Labeled = p
    let s: String = p.label
    print("labeled", q.x, s, l.label)
  else
    print("plain", p.x)
  end
end

fn take_either(v: P | Q)
  if v is Labeled
    let w: P | Q = v
    print("labeled", v.label)
  else
    print("plain")
  end
end

fn take_any(v: any)
  if v is Labeled
    print(v.label)
  else
    print("not labeled")
  end
end

take(P { x: 1 })
take(Kid { x: 2 })
take_either(Q { y: 3 })
take_either(Kid { x: 4 })
take_any(P { x: 5 })
take_any(Kid { x: 6 })
