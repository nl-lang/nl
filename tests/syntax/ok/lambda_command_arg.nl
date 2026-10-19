# A short function written with a name is a command
# argument; `f x -> e` is `f(x -> e)`, and a block after it goes to the
# command call.
each items, x -> print(x)
f x -> x + 1
each items, x -> x do |y| y end
