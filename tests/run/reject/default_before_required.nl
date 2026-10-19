# Rejected by the compiler.
# expect-error: parameters with defaults come after those without
fn f(a = 1, b)
  a + b
end

print f(1, 2)
