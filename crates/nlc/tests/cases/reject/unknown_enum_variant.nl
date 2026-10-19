# An enum has its variants, `values` and `name`; any other
# member is an error, as in Rust and TypeScript, not a nil at runtime.
enum Color
  Red
end
print(Color.Red, Color.values.len, Color.name("Red"))
# expect-error: no variant 'Blue'
print(Color.Blue)
