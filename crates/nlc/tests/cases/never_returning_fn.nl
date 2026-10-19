# `raise` returns `never`, and so does a function whose body
# always raises; its call is no `nil` operand.
fn boom(a)
  raise "x"
end

fn boom_finally(a)
  try
    raise "y"
  finally
    print("finally")
  end
end

fn boom_caught(a)
  try
    raise "z"
  catch e
    raise "again: #{e.message}"
  end
end

try
  print(boom(1) >= 2)
catch e
  print(e.message)
end
try
  print(boom_finally(1) + 1)
catch e
  print(e.message)
end
try
  print(-boom_caught(1))
catch e
  print(e.message)
end
