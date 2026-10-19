# A file returns its export table; a top-level `return` has nothing to
# return from.
fn f
  1
end
# expect-error: `return` at the top level of a file
return f()
