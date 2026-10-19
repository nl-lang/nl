# raise and catch; a raised string becomes an Exception
fn fail
  raise "error here"
end

try
  fail()
catch err
  print "caught: #{err.message}"
  print typeof(err)
end

try
  raise Exception { message: "built" }
catch err: Exception
  print err.message
end

try
  fail()
catch
  print "bare catch"
end

fn never_returns(msg: String): never
  raise msg
end

try
  never_returns("from never")
catch e
  print e.message
end
print "after"
