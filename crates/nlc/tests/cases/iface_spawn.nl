# The methods of an interface implementation are named
# functions, so `spawn x.method()` can send them.
interface Shape
  fn area: Number
end
struct Sq
  side: Number
end
interface Shape for Sq
  fn area: Number
    @side * @side
  end
end
let s = Sq { side: 3 }
let task = spawn s.area()
task.on_done do |v|
  print "area", v
end
