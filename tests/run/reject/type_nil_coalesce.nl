# Rejected by the analyzer: `n ?? 0` is a Number whatever `n` holds.
fn label(n: Number?)
  # expect-error: `n ?? 0` is a Number, never the String the variable is declared to hold
  let s: String = n ?? 0
  s
end
print label(1)
