# Recovery: garbage inside a parameter list; the other parameters and the
# body parse.
fn f(a, 1 + 2, b: Number)
  a + b
end
y = 1
