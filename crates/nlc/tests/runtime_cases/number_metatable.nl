# Under `nlc run` numbers and booleans get their metatables, so
# Lua code reaches Number and Bool methods, and operator definitions on
# Number reach a right operand that has no metamethod.
fn Number.half
  self / 2
end
fn Bool.flip
  not self
end
fn Number.*(other)
  "Number * #{typeof(other)}"
end
let lua = include("lib/number_calls.lua")
print lua.half(9)
print lua.flip(true)
print lua.floor(2.5)
let t: any = {}
print 2 * t
print 2 * 3
