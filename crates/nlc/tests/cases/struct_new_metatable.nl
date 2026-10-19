# checked-only
# backends: lua,bc
# A struct test reads the raw metatable in the constructor
# that the runtime makes for a struct's literals as well (runtime/
# nl_20_struct.lua): with a `__metatable` field on the struct, its
# instances are still instances, and a table whose metatable names the
# struct in `__metatable` is not one, the first time a literal runs and
# every time after (struct_metatable_field.nl has the inline literal).
struct S
  x: Number
end
struct Box
  s: S?
  t: S | Number
end
# A literal through an alias is built by the runtime.
type B = Box

let real = S { x: 1 }
:rawset(S, "__metatable", "locked")
let spoof: any = setmetatable({}, {__metatable: S})

fn field(v)
  try
    typeof(B { s: v, t: v }.t)
  catch e: TypeError
    e.message
  end
end
for _i in 1..3
  print field(real), field(spoof)
end
