# Declared host types are registered against gmod's metatables, so checks and typeof see them
fn describe(e: Entity): String
  "#{typeof(e)} #{e.get_class} #{e.ent_index}"
end

fn greet(p: Player): String
  "hello #{p.nick}"
end

let bob = player_by_id(1)
print greet(bob)
print describe(bob)
print describe(entity_by_index(10))
print describe(entity_by_index(11))
print(bob is Entity, bob is Player, entity_by_index(10) is Player)

let prop: any = entity_by_index(10)
try
  greet(prop)
catch e: TypeError
  print "TypeError: #{e.message}"
end
