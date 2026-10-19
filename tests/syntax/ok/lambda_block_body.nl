# `-> do … end` is a function with statements, which
# returns what `return` returns; after `->` the body may start on the next
# line.
let minmax = |a, b| -> do
  return a, b if a < b
  return b, a
end
let later = x ->
  x + 1
let block = x ->
  do print(x) end
