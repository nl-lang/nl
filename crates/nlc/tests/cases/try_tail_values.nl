# Finding 16: a `try` that is a function's tail returns every value of a
# final call, as `if` and `case` do.
fn pos
  return 1, 2
end
fn tail_if
  if true
    pos()
  end
end
fn tail_try
  try
    pos()
  catch
    0
  end
end
fn tail_catch
  try
    raise "x"
  catch
    pos()
  end
end
fn tail_nested
  try
    try
      pos()
    finally
    end
  catch
    0
  end
end
fn tail_none
  try
    let a = 1
  catch
    0
  end
end
print tail_if()
print tail_try()
print tail_catch()
print tail_nested()
print tail_none()
let u, w = tail_try()
print u, w
