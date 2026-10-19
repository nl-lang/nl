# Catch clauses are tried in order with `is`; unmatched errors continue outward
struct MyException : Exception
  reason: String
end

struct OtherException : Exception
end

fn risky(kind)
  if kind == 1
    raise MyException { reason: "i am not a clever fox" }
  elseif kind == 2
    raise OtherException { message: "other" }
  end
  raise "plain"
end

fn handle(kind)
  try
    risky(kind)
  catch err: MyException
    print "my: #{err.reason}"
  catch err: OtherException
    print "other: #{err.message}"
  catch err: Exception
    print "exception: #{err.message}"
  end
end

handle(1)
handle(2)
handle(3)

try
  try
    risky(2)
  catch err: MyException
    print "not this one"
  end
catch err
  print "outer caught #{typeof(err)}"
end

let e = MyException { reason: "r" }
print(e is Exception)
print e.message == ""
