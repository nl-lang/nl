# In a `Function(…)` type every `:` is the return type; the
# Lua-path reading is only for `fn` headers and block parameters.
type F = Function(a: Number) :Number
type G = Function() : nil
