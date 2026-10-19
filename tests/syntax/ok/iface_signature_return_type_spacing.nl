# A signature has no body, so every `:` after it is its
# return type, whatever the spacing.
interface I
  fn a() :Number
  fn b :Number
end
