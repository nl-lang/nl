# `x -> expr`, one plain name before `->`; lambdas
# nest to the right.
let f = x -> x + 1
let add = a -> b -> a + b
g(x -> x * 2)
