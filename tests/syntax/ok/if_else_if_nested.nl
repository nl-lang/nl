# `else if` is an `else` whose block starts with a nested `if`,
# which needs its own `end`.
if a
  x()
else if b
  y()
end
end
