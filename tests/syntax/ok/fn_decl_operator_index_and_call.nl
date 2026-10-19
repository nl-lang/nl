# Operator names `[]`, `[]=` and `()`; the `()` of the call operator
# is part of the name, so `fn Foo.()(*args)` has the parameter list `(*args)`.
fn Foo.[](key)
end
fn Foo.[]=(key, value)
end
fn Foo.()(*args)
end
