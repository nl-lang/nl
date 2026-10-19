# Rejected by the compiler.
fn positive(a): Bool
  if a > 0
    # expect-error: the `if` is not the last statement, so this value is discarded, and a bare `true` is not a call or an assignment
    true
  end
  false
end

print positive(1)
