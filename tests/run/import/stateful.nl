# Helper for import_once.nl: top-level state that must exist once per state.
print "stateful loaded"
let count = 0

pub fn bump
  count += 1
  count
end

pub fn current
  count
end
