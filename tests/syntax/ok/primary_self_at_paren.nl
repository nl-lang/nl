# `self`, `Self`, `@name` and a parenthesized expr.
fn Foo.m(*args)
  x = self
  x = Self
  x = @hp
  x = (a + b)
  args
end
