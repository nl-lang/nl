# A `const` directly in a module body is a member of the module, read as
# `Self.limit` inside and `M.limit` outside; `let` stays
# private.
module Inventory
  const limit = 10
  const MAX_STACK = 64
  let slots = []
  fn room
    Self.limit - slots.len
  end
end
print Inventory.limit, Inventory.MAX_STACK, Inventory.room
print Inventory.slots == nil
module Inventory
  fn cap
    Self.limit * Self.MAX_STACK
  end
end
print Inventory.cap
