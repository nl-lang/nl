# `return` with nothing, one value, several values; `return`
# before `end`.
fn f
  return
end
fn g
  return 1
end
fn h
  return a, b
end
fn k
  if x; return end
end
