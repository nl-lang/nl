# The callee of `spawn` ends in a lowercase name; a Lua
# path, a method call or an index is not a call_path.
a = spawn Foo(x)
b = spawn self(x)
c = spawn :lua.f(x)
d = spawn x:m(1)
e = spawn t[1](x)
