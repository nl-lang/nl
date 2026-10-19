# Interfaces with methods, Self and a required field; conformance is nominal
interface Shape
  fn area: Number
  fn scale(factor: Number): Self
  name: String
end

struct Circle
  r: Number
  name: String = "circle"
end

struct Blob
  name: String = "blob"
end

interface Shape for Circle
  fn area: Number
    3 * @r ** 2
  end

  fn scale(factor: Number): Self
    Circle { r: @r * factor, name: @name }
  end
end

fn describe(s: Shape): String
  "#{s.name} #{s.area}"
end

let c = Circle { r: 1 }
print c.area
print c.scale(2).area
print(c is Shape)
print(Blob { name: "b" } is Shape)
print describe(c)
let b: any = Blob { name: "b" }
try
  describe(b)
catch err: TypeError
  print "Blob is not a Shape"
end
