# A parameter name is declared once in a parameter list,
# also inside a destructuring parameter.
# expect-error: `a` is declared twice
fn f(a, {b, a})
  a
end
print f(1, {b: 2, a: 3})
