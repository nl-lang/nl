# A struct field is declared once.
struct P
  x: Number
  # expect-error: `x` is declared twice
  x: String
end
