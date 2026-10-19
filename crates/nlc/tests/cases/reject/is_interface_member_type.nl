# A struct narrowed by `is I` is `Q & I`: the members the
# interface declares keep their types in the branch.
interface Labeled
  fn label: String
end
struct Q
  y: Number
end
fn f(q: Q)
  if q is Labeled
    let n: Number = q.y
# expect-error: expects Number, got String
    let s: Number = q.label
  end
end
