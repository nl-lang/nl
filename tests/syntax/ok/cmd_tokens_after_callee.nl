# After a callee `-`, `*`, `..`, `...`, `(`, `[` and binary
# operators continue the expression; `!x` and `~x` start command arguments.
x = f -1
x = f *y
x = f ..y
x = f ...y
x = f [1]
f !x
f ~x
