# Helper for conversion_multi.nl: a second `To` for Number, from a file of
# its own (the `to` dispatcher is shared between files).
pub struct Feet
  value: Number
end
interface To<Feet> for Number
  fn to: Feet
    Feet { value: self * 3 }
  end
end
