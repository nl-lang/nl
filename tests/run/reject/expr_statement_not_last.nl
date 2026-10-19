# Rejected by the compiler.
fn f(a, b)
  # expect-error: an expression statement that is not last must be a call or an assignment; `a + b` is neither
  a + b
  print "after"
end

f(1, 2)
