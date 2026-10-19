# The pipe target: a name, `@name`, a member chain, a call
# with arguments, a method call, a Lua path, a trailing block.
x = a |> f
x = a |> @handler
x = a |> x.y.m
x = a |> f(b)
x = a |> x:m(b)
x = a |> :string.upper
x = a |> Mod.sub.fn_name
x = items |> each do |i| print i end
x = items |> filter(alive) |> map(name_of)
