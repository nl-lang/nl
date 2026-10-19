# Arithmetic, negation and concatenation operators on a struct
struct Vec
  x: Number
  y: Number
end

fn Vec.+(right)
  Vec { x: @x + right.x, y: @y + right.y }
end

fn Vec.-(right)
  Vec { x: @x - right.x, y: @y - right.y }
end

fn Vec.-
  Vec { x: -@x, y: -@y }
end

fn Vec.*(right)
  Vec { x: @x * right, y: @y * right }
end

fn Vec./(right)
  Vec { x: @x / right, y: @y / right }
end

fn Vec.%(right)
  Vec { x: @x % right, y: @y % right }
end

fn Vec.**(right)
  Vec { x: @x ** right, y: @y ** right }
end

fn Vec.//(right)
  Vec { x: @x // right, y: @y // right }
end

fn Vec.<>(right)
  "(#{@x},#{@y})" <> right
end

fn Vec.show
  "(#{@x},#{@y})"
end

let a = Vec { x: 1, y: 2 }
let b = Vec { x: 10, y: 20 }
print((a + b).show)
print((b - a).show)
print((-a).show)
print((a * 3).show)
print((b / 4).show)
print((b % 3).show)
print((a ** 2).show)
print((b // 3).show)
print(a <> "!")

struct Named : Vec
  label: String = "n"
end

let n = Named { x: 5, y: 5 }
print((n + a).show)

let acc = Vec { x: 0, y: 0 }
acc += a
acc += a
print acc.show

struct Plain
  v: Number
end

let p1: any = Plain { v: 1 }
let p2: any = Plain { v: 2 }
try
  let sum = p1 + p2
catch
  print "no + for Plain"
end
