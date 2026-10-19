# The type of `catch` is a type; a lowercase name other
# than nil, any, unknown and never is not one.
try
  f()
catch e: lower
  g()
end
