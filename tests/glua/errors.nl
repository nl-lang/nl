# Mistakes the declarations catch: each line below the first is an error
# (the test lists them by line).

let ply = local_player()
ply.set_health "full"
Net.write_uint "many", 16
let name: Number = ply.nick
Timer.simple "soon", fn
  print "late"
end
ply.give 5
Surface.draw_rect 0, 0, 10
let v: Angle = ply.get_pos
print v, name
