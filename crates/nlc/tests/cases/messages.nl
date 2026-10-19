# checked-only
# Messages of runtime checks name what failed: the field of a pattern, the
# variable initialized with `fn`, the destructured parameter.
struct Monster
  name: String
  level: Number
end
fn describe({name, level}: Monster)
  "#{name} #{level}"
end
let bad: any = {name: "x"}
try
  describe(bad)
catch e
  print e.message
end
fn hp({hp: Number})
  hp
end
try
  hp({hp: "no"})
catch e
  print e.message
end
let f = fn(x) x end
let g: any = 5
try
  f = g
catch e
  print e.message
end
try
  let {pos: {x: Number}} = {pos: {x: "s"}}
catch e
  print e.message
end
try
  let [a: Number, b: String] = [1, 2]
catch e
  print e.message
end
