# Fields with a type, a default, both, or neither.
struct Monster
  id: Number
  name: String = "unnamed"
  count = 0
  tag
  next: Self?
end
