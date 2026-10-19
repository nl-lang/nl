import { describe } from 'util'

let units: [Unit] = []
let elapsed = 0

pub fn start(count: Number)
  for i in 1..count
    Table.insert(units, Engine.new_unit("orc #{i}", 10 * i))
  end
  Engine.log "spawned #{units.len} units"
end

on Game.Tick do |dt: Number|
  elapsed += dt
  for u in units
    u.damage 5 if u.hp > 0
  end
end

on UnitDied as 'mourn' async do |u: Unit|
  Task.sleep(0.5).await
  Engine.log "#{describe(u)} is gone at #{elapsed}s"
end
