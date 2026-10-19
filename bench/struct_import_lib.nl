# The structs of struct_import.nl, which another file constructs.
pub struct Vec
  x: Number
  y: Number
end

pub struct Thing
  id: Number
  tag: String?
end

pub struct Body : Thing
  pos: Vec
  mass: Number = 2
end
