# Helper for worker_type_values.nl: an enum, a module and an interface. No top-level effects.
pub enum Shade
  Light
  Dark
end

pub module Tools
  fn double(x)
    x * 2
  end
end

pub interface Named
  fn name
end
