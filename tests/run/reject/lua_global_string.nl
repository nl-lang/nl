# Rejected by the resolver.
# expect-error: Lua's `string` is visible in NL files only as the Lua path `:string`
print string.len("abc")
