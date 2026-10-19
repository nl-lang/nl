# `x:m(a)`, `x:m` (the call `x:m()`), and a trailing
# block after a method call.
x:m(a)
x:m
y = x:m
x:each do |v| end
x:m(1):n(2)
