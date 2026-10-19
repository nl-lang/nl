# Finding 10: in a keyed pattern, a dotted type name binds the key and
# checks the type; an enum variant only compares.
module Geo
end
struct Pt
  x: Number
end
Geo.P = Pt
enum Dir
  North
  South
end
let v = {d: "North", p: Pt {x: 1}}
if let {d: Dir.North, p: Geo.P} = v
  print "matched", p.x
end
if let {p: Geo.P} = {p: 5}
  print "wrong"
else
  print "not a Geo.P"
end
let {p: Geo.P} = v
print p.x
case "South"
when Dir.North
  print "north"
when Dir.South
  print "south"
end
