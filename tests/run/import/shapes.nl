# Helper for the struct_new_ tests: structs that other files construct, and a record of the defaults that ran.
let log = []

fn note(tag, value)
  Table.insert(log, tag)
  value
end

# The defaults evaluated since the last call, in order.
pub fn noted
  let text = Table.concat(log, " ")
  log = []
  text
end

pub struct Vec
  x: Number
  y: Number
end

fn Vec.sum
  @x + @y
end

pub struct Tagged : Vec
  name: String = "tag"
end

pub struct Entity
  id: Number
  name: String = note("name", "unnamed")
end

pub struct Monster : Entity
  level: Number = note("level", 1)
  boss: Bool?
  pos: Vec?
  loot = note("loot", "none")
end

fn Monster.describe
  "#{@name}/#{@id}/#{@level}/#{@boss}/#{@loot}"
end

pub enum Mode
  Easy
  Hard
end

pub struct Game
  mode: Mode
  seed: Number | String
  first: Entity
end
