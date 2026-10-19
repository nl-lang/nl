# Records are structural types for plain tables
type Point = {x: Number, y: Number}

fn norm1(p: Point): Number
  p.x + p.y
end

let p: Point = {x: 1, y: 2}
print norm1(p)
print norm1({x: 3, y: 4, z: 5})
print({x: 1, y: 2} is Point)
print({x: 1} is Point)
print({x: 1, y: "2"} is Point)
let missing: any = {x: 1}
try
  norm1(missing)
catch err: TypeError
  print "missing field: TypeError"
end
