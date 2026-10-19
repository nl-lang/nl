# Single inheritance; Self stays the parent in an inherited method
struct Entity
  id: Number
  name: String = "unnamed"
end

struct Monster : Entity
  level: Number = 1
end

fn Entity.label
  "#{@name}/#{@id}"
end

fn Entity.copy_base
  Self { id: @id, name: @name }
end

fn Monster.describe
  "#{@name} (level #{@level})"
end

fn show(e: Entity)
  e.label
end

let m = Monster { id: 1, name: "bat" }
print m.describe
print m.label
print show(m)
print(m is Monster)
print(m is Entity)
let e = Entity { id: 2 }
print(e is Monster)
print typeof(m)
let base = m.copy_base
print typeof(base)
print m.level
