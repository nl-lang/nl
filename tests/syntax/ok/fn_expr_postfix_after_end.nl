# Postfix operators may follow the `end` of a block
# expression.
x = fn() 1 end()
y = (if a; b else c end).name
