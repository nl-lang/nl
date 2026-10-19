# `f [1, 2]` is an index; a list argument needs parentheses.
fn f(l)
  l
end
# expect-error: to pass a list, write
f [1, 2]
