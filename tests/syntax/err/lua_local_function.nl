# `local f = function(x) … end` is Lua; one error for both words,
# and the function's `end` closes the function.
local f = function(x)
  x
end
print f(1)
