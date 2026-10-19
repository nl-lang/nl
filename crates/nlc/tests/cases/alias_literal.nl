# `A { … }` with `A` a `type` alias of a
# struct constructs the struct, as it does through an imported alias; also
# through a chain of aliases and with type arguments.
struct B
  x: Number
  y: Number = 7
end
type A = B
type AA = A
let a = A { x: 1 }
print "#{a.x} #{a.y} #{a is B} #{typeof(a)}"
let aa = AA { x: 2 }
print "#{aa.x} #{aa is A}"
struct Box<T>
  v: T
end
type NumBox = Box<Number>
print(NumBox { v: 3 }.v)
type Problem = Exception
let p = Problem { message: "boom" }
print "#{typeof(p)} #{p.message}"
