# checked-only
# Finding 7: the declared return type is checked when a function ends
# without returning, and `return v` in a `: nil` function is checked.
fn f: Number
  let x = 1
end
fn g(a): Number
  if a
    1
  end
end
fn ok(a): Number?
  if a
    1
  end
end
fn h: nil
  return 5
end
fn h2: nil
  return
end
for test in [f!, fn() g(false) end, fn() h() end]
  try
    test()
  catch e
    print typeof(e), e.message
  end
end
print g(true), ok(false), h2()
