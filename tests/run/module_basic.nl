# Modules, private state, sibling calls, nesting and reopening
module Inventory
  let slots = []

  fn add(item)
    slots[slots.len + 1] = item
  end

  fn count
    slots.len
  end

  fn describe
    "#{count} items"
  end

  fn names
    Table.concat(slots, ",")
  end
end

Inventory.add "sword"
Inventory.add("shield")
print Inventory.count
print Inventory.describe
print Inventory.names

module Inventory.Filters
  fn longer_than(list, n)
    let out = []
    for v in list
      Table.insert(out, v) if v.length > n
    end
    out
  end
end

print Table.concat(Inventory.Filters.longer_than(["a", "abcd", "xyz"], 2), ",")

module Inventory
  fn reopened
    "reopened with #{self.count} items"
  end
end

print Inventory.reopened
print Inventory.count
