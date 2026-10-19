# Rejected by the compiler.
struct Entity
  id: Number
end

struct Monster : Entity
  # expect-error: a child cannot redeclare a parent's field
  id: Number
end
