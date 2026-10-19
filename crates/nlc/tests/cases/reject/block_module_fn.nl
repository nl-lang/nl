# A block is passed only to a function with a `Function` parameter;
# a module function's parameters are visible to the compiler.
module M
  fn plain(x)
    x
  end
end
fn M.later(n: Number)
  n
end
fn M.takes(cb: Function)
  cb()
end
M.takes do
  1
end
# expect-error: `M.later` has no Function parameter
M.later 1 do
end
