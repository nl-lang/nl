# The type after `is` must be checked at
# runtime; a type parameter is not, also as a member
# of a union.
fn g<T>(a)
  # expect-error: `is` with a type parameter
  a is T | Number
end
print g(1)
