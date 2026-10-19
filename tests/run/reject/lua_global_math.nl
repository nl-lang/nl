# Rejected by the resolver.
# expect-error: `math` is not an NL name; use `Number.floor(x)`, `x.floor` or the Lua path `:math.floor(x)`
print math.floor(2.5)
