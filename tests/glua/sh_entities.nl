# Shared: traces, entity searches, vectors and angles, timers, convars
# and enums.

let range = create_con_var("nl_range", "512", FCVAR_ARCHIVE | FCVAR_REPLICATED, "How far to look")

fn props_near(pos: Vector): [Entity]
  let found = []
  for ent in Ents.find_in_sphere(pos, range.get_float)
    found.push ent if ent.get_class == "prop_physics"
  end
  found
end

fn look_at(ply: Player): Entity?
  let start = ply.get_shoot_pos
  let dir = ply.get_aim_vector
  let tr = Util.trace_line({start: start, endpos: start + dir * range.get_int, filter: ply, mask: MASK_SHOT})
  return tr.Entity if tr.Hit
  nil
end

fn push_away(ent: Entity, from: Vector)
  let phys = ent.get_physics_object
  return unless phys.is_valid
  let dir = ent.get_pos - from
  dir.normalize
  phys.apply_force_center dir * phys.get_mass * 500
end

let ang = angle(0, 90, 0)
let fwd = ang.forward
print "forward #{fwd}, yaw #{ang.yaw}, length #{fwd.length}, dot #{fwd.dot(vector(1, 0, 0))}"

Timer.create "nl_tick", 1, 0, fn
  for ply in PlayerLib.get_all()
    let ent = look_at(ply)
    if ent and ent.is_valid
      push_away ent, ply.get_pos
      ply.emit_sound "physics/metal/metal_box_impact_hard1.wav", 75, 100
    end
    for prop in props_near(ply.get_pos)
      prop.set_color color(255, 0, 0)
    end
  end
end

print "tick every #{Timer.time_left("nl_tick")}s; #{Engine.tick_interval()}"
print "solid: #{MASK_SOLID}, conditions: #{COND.NONE}, cur time #{cur_time()}"
