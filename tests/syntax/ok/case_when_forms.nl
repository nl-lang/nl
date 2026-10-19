# The `when` items: an expression, a range, a brace or
# bracket pattern, `is T`, a struct pattern, several items.
case x
when 1, 2
  a()
when 3..9
  b()
when {role: "user"}
  c()
when [x, 456]
  d()
when is Number | String
  e()
when Circle {r}
  f()
when Direction.North
  g()
else
  h()
end
