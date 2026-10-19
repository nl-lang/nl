# Postfix `!` after a call, an index, a Lua path, a
# method call is an error.
a = f()!
b = t[1]!
c = :lua.path!
d = x:m!
