# NL has no varargs; `...` is only the range operator. As
# a parameter, as a value, as a command argument and in a `Function` type it
# is reported with the `*args` form; the rest of each function parses.
fn f(...)
  g(...)
  let t = [...]
  g ...
end
let h: Function(Number, ...): nil
