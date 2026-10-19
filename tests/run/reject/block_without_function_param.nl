# Rejected by the compiler.
fn twice(x)
  x * 2
end

# expect-error: a block can only be passed to a function that has a `Function` parameter
twice 2 do |v|
  print v
end
