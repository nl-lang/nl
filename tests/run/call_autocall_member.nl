# x.member is called when its value is a method, or when the statement discards it
struct Player
  name: String
  x: Number = 0
end

fn Player.distance
  @x * 2
end

fn Player.step
  @x += 1
end

let p = Player { name: "ann", x: 3 }
let n = p.name
let d = p.distance
print n
print d
p.step
print p.x
let m = p.distance!
print typeof(m)
print m()
print p.distance()

let button = {on_click: fn(x) x * 10 end}
let handler = button.on_click
print typeof(handler)
print button.on_click(4)

let hits = {n: 0}
let obj = {hit: fn() hits.n += 1 end}
obj.hit
obj.hit
print hits.n
