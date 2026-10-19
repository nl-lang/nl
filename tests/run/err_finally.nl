# finally runs after the body or the chosen catch, and when the error continues outward
fn attempt(should_fail)
  try
    print "body"
    raise "boom" if should_fail
    print "body done"
  catch err
    print "catch #{err.message}"
  finally
    print "finally"
  end
end

attempt(false)
attempt(true)

struct Unrelated : Exception
end

try
  try
    raise "inner"
  catch err: Unrelated
    print "not matched"
  finally
    print "inner finally"
  end
catch err
  print "outer #{err.message}"
end
