# A struct whose metatable is locked with `__metatable` is still
# its type (the runtime uses the raw metatable).
struct Locked
  x: Number = 1
end
let lock = include("lib/lock.lua")
let v = Locked {}
lock(Locked)
print typeof(v), v is Locked
fn take(p: Locked)
  p.x
end
print take(v)
