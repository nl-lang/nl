# The result of a `To` or `From` conversion must
# be a value of the target type; one that is not raises
# ConversionError, for `as`, `tostring` and interpolation alike. (The `to`
# and `from` methods carry no return type, so the runtime checks of a
# checked build do not catch it first.)
struct N
  v: Number
end
interface To<Number> for N
  fn to
    "not a number"
  end
end
try
  print(N { v: 1 } as Number)
catch e: ConversionError
  print "as Number: #{e.message}"
end

struct Z
  v: Number
end
interface To<String> for Z
  fn to
    42
  end
end
let z = Z { v: 1 }
try
  print "#{z}"
catch e: ConversionError
  print "interpolation: #{e.message}"
end
try
  print tostring(z)
catch e: ConversionError
  print "tostring: #{e.message}"
end
try
  print(z as String)
catch e: ConversionError
  print "as String: #{e.message}"
end

struct C
  d: Number
end
interface From<C> for Bool
  fn from(c)
    "yes"
  end
end
try
  print(C { d: 1 } as Bool)
catch e: ConversionError
  print "From: #{e.message}"
end

interface To<C> for Number
  fn to
    nil
  end
end
try
  print(5 as C)
catch e: ConversionError
  print "nil result: #{e.message}"
end

# Conversions that give a T still work.
struct Good
  v: Number
end
interface To<String> for Good
  fn to
    "Good(#{@v})"
  end
end
let g = Good { v: 2 }
print "#{g} #{tostring(g)} #{g as String}"
interface From<Good> for Number
  fn from(x)
    x.v * 10
  end
end
print(g as Number)
