# Rejected by the compiler.
struct Bag
  # expect-error: `len` is a reserved member name and cannot be a struct field
  len: Number
end
