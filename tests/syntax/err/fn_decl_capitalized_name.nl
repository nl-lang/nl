# `fn` and a capitalized name followed by `(` is read as a
# declaration with a misnamed function, not as a function whose body calls it.
fn Foo(x)
  x
end
