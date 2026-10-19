# Structs, field defaults and construction checks
let counter = 0
fn next_id
  counter += 1
  counter
end

struct Entity
  id: Number = next_id()
  name: String = "unnamed"
end

let a = Entity { name: "ay" }
let b = Entity { name: "bee" }
let c = Entity { id: 50, name: "sea" }
print "#{a.id} #{a.name}"
print "#{b.id} #{b.name}"
print "#{c.id} #{c.name}"
print counter
let d = Entity { id: 7 }
print "#{d.id} #{d.name}"
a.name = "renamed"
print a.name
print typeof(a)
print(a is Entity)
print({id: 1, name: "x"} is Entity)
print :type(a)
let bad: any = "one"
try
  let e = Entity { id: bad }
catch err: TypeError
  print "TypeError"
end
