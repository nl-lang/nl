# The first name of a Lua path binds the NL function it is written in, not the Lua global of that name
:_G.greet = fn(name) "the host greets #{name}" end

# A wrapper of the host function reaches it through `_G`
fn greet(name)
  "wrapped: " <> :_G.greet(name)
end

print greet("ann")

# `:countdown` is this function itself: the call recurses (and is linted)
fn countdown(n)
  if n == 0
    return "liftoff"
  end
  print n
  :countdown(n - 1)
end

print countdown(3)
