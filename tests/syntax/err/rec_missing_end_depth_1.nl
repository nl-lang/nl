# Recovery: an `fn` lacks its `end`; the indentation of the next `fn` shows
# where it was missing, and that `fn` parses normally.
fn a()
  foo()

fn b()
  bar()
end
