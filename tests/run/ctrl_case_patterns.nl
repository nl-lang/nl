# case with pattern items, Range values and guards
struct Circle
  r: Number
end

fn kind(msg)
  case msg
  when 3..9
    "range"
  when is String
    "string"
  when {kind: "move", to: dest}
    "move to #{dest}"
  when Circle {r} if r > 10
    "big circle"
  when Circle {r}
    "circle #{r}"
  when [x, 456]
    "pair starting #{x}"
  else
    "other"
  end
end

print kind(5)
print kind(10)
print kind("hi")
print kind({kind: "move", to: "home"})
print kind({kind: "stop"})
print kind(Circle { r: 20 })
print kind(Circle { r: 2 })
print kind([1, 456])
print kind([1, 456, 7])

let r = 1..5
fn in_range(n)
  case n
  when r
    "inside"
  else
    "outside"
  end
end

print in_range(3)
print in_range(6)

let me = "kim"
fn owner(item)
  case item
  when {owner: o} if o == me
    "mine"
  when {owner: o}
    "owned by #{o}"
  end
end

print owner({owner: "kim"})
print owner({owner: "lee"})
