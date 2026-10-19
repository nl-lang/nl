# Recovery: a run of `async` not followed by `fn` or `do` is one mistake: one
# error, and the operand after it is read as usual.
x = async async y + 1
z = 2
