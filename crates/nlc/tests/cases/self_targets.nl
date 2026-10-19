# Findings 14 and 15: `Self` in `fn A.B.name` is the nested module `A.B`,
# and in `fn Bool.name` it is the table of Bool's methods.
module Inv
end
module Inv.Filters
end
fn Inv.Filters.late
  Self
end
print Inv.Filters.late == Inv.Filters
fn Bool.flip
  not self
end
fn Bool.get_flip
  Self.flip!
end
let f = true.get_flip
print typeof(f), f(true), f(false)
fn Number.get_half
  Self.half!
end
fn Number.half
  self / 2
end
let h = 3.get_half
print h(10)
