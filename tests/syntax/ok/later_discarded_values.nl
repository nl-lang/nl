# An expression statement that is not last and not a call or an
# assignment parses; the discarded-value rule is checked on the tree.
fn f
  true
  a + b
  x!
  Foo { a: 1 }
  [1, 2]
  if a
    true
  end
  1
end
