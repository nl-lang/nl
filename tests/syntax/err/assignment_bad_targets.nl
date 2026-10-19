# A call, a method call, a literal, `self`, `Self`, `x!` and a
# Lua path without a step are not targets.
f() = 1
x:m = 1
1 = 2
self = 1
Self = 1
a! = b
:x = 1
