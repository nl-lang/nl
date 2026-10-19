# Items of one `when` that bind different names would leave some
# of them unbound; rejected.
fn f(v)
  case v
  # expect-error: the second item binds `y`, the first `x`
  when [x, 1], [1, y]
    x
  end
end
print f([1, 1])
