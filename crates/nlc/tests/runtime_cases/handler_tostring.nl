# An error value whose __tostring raises, or returns something
# other than a string, is still caught as a LuaError.
let bad = include("lib/bad_errors.lua")
fn raise_lua(v)
  try
    :error(v)
  catch e: Exception
    print typeof(e), e.message, e.value == v
  end
end
raise_lua(bad.raising)
raise_lua(bad.nonstring)
try
  raise bad.raising
catch e: Exception
  print "raise:", typeof(e)
end
