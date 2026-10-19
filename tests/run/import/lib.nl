# Helper for the import_ tests: one exported item of every kind. Prints once when it runs.
print "lib loaded"

pub fn square(x)
  x * x
end

pub fn answer
  42
end

pub const limit = 10

pub enum Mode
  Easy
  Hard
end

pub struct Point
  x: Number
  y: Number
end

fn Point.sum
  @x + @y
end

pub type Pair = {a: Number, b: Number}

pub interface Named
  fn name_of: String
end

pub module Tools
  fn twice(x)
    x * 2
  end
end

fn private_helper
  "private"
end
