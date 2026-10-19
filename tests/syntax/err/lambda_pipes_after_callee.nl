# After a callee `|` and `||` are operators, so a short
# function with pipes does not start a command argument.
f |a| -> a
f || -> 1
