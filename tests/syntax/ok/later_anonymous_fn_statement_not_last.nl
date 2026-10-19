# An anonymous function as a statement that is not last
# parses; the discarded-value rule rejects it.
fn f
  fn(x) x end
  g()
end
