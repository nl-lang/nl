# A `when` item is a pattern when it starts with `{`, `[`, `is`
# or a capitalized path followed by `{`; otherwise an expression.
case v
when x
  a()
when Direction.North
  b()
when Circle
  c()
when 3..9, lo..hi
  d()
when re
  e()
when Circle {r}
  f()
when {a}, [b], is T
  g()
end
