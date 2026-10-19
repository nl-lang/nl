# An index of a `String?` is typed from its String member, not `any`.
fn pick(o: String?)
# expect-error: expects Number, got String
  let n: Number = o[1..2]
end
