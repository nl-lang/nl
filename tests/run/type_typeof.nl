# typeof gives NL type names; Lua's type is reached as :type
struct Point
  x: Number
end

fn noop
end

print typeof(1)
print typeof("s")
print typeof(true)
print typeof({})
print typeof(noop!)
print typeof(Point { x: 1 })
print :type(1)
print :type("s")
print :type(Point { x: 1 })
