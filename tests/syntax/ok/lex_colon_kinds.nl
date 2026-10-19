# Method call, Lua path, annotation, key and return-type
# colons; after an fn header `: T` is a return type (the Lua-path body
# `fn g() :x()` warns, see err/).
conn:send("x")
:print(1)
let a :Number = 1
t = {k :1}
fn f(): T
end
fn g(): T
  :lua_call()
end
