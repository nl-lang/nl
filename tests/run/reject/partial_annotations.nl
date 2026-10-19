# Rejected by the compiler.
# expect-error: if any parameter is annotated, all must be
fn f(a: Number, b)
  a
end

print f(1, 2)
