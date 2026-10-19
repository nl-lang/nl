# In `if ready then` the name `then` is unexpected;
# the error carries the hint for Lua users, and the body on the next line
# still parses.
if ready then
  go()
end
