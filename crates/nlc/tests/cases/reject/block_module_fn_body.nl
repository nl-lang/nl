# The same for a function declared in the module body.
module M
  fn plain(x)
    x
  end
end
# expect-error: `M.plain` has no Function parameter
M.plain 1 do
end
