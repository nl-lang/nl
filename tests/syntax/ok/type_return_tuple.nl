# `(A, B)` after the colon lists several return values; a
# `(T)` with one type still groups. Allowed on fn declarations, anonymous
# functions, blocks, Function signatures and interface members.
fn pair(): (Number, String)
  1, "a"
end
fn one(): (Number)
  1
end
let f = fn(): (Number, String) 1, "a" end
each do |x|: (Number, Number) x, x end
let g: Function(a: Number): (Number, String)
interface Splitter
  fn split(s: String): (String, String)
end
