# A module nested in a declared module is a declared module too: a member
# with `self` is called with the nested table as `self`, a member without
# it gets none, and every name is looked up under its Lua key, the
# nested module's own `as` key included.
# decl: lib/host_api.d.nl
include "lib/host_api.lua"

print Top.Sub.meth("a")
print Top.Sub.plain("b")
print Top.Inner.hello()
print Top.Inner.count
let inner = Top.Inner
print inner.hello() <> "!"
