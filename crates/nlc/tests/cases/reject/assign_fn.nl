# A name declared with `fn` is a named function;
# assigning to it is an error. `let f = fn … end` is the replaceable form.
fn foo
  1
end
# expect-error: `foo` is declared with `fn`
foo = 5
print foo
