# A struct's fields and methods share one namespace.
struct P
  name: String
end
# expect-error: the field `name` would hide this method
fn P.name
  "method"
end
