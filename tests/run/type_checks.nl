# Runtime checks on annotated positions raise TypeError
fn half(n: Number): Number
  n / 2
end

fn bad_return(x: any): Number
  x
end

let wrong: any = "text"

try
  half(wrong)
catch err: TypeError
  print "parameter: #{typeof(err)}"
end

try
  bad_return(wrong)
catch err: TypeError
  print "return: #{typeof(err)}"
end

let count: Number = 1
try
  count = wrong
catch err: TypeError
  print "assignment: #{typeof(err)}"
end
print count

try
  let flag: Bool = wrong
catch err: TypeError
  print "let: #{typeof(err)}"
end

try
  half(wrong)
catch err: Exception
  print(err is TypeError)
end
print half(4)
