# Recovery: an `if` inside an `fn` lacks its `end`; the `end` at the `fn`'s
# column closes the `fn`, and the next declaration parses normally.
fn a()
  if x
    foo()
end

fn b()
  bar()
end
