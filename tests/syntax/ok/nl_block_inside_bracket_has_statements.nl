# An `fn … end` inside parentheses pushes a statement context:
# its line breaks separate statements again.
f(fn(x)
  y = x
  -y
end)
