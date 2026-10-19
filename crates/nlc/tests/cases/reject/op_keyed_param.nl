# An operator method has a fixed number of parameters; `**opts` is not one of them.
struct V
  x: Number
end
# expect-error: one parameter
fn V.+(right: V, **opts: any): V
  V { x: @x + right.x }
end
