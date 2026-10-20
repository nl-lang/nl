# a declared Lua class is checked by its metatable: looked up until it exists, then kept
add_path("import/decl")
let classes = include("include/lua_class.lua")
let lib = include("include/declared_class.nl")
print lib.attempt({x: 1})
print lib.attempt(nil)
print "lookups before the class exists: #{classes.lookups()}"
classes.register("Vec")
let v = classes.new("Vec", 3)
print lib.attempt(v)
let sum = 0
for _ in 1..100
  sum += lib.x_of(v)
end
print sum
print lib.attempt({x: 1})
print "lookups after: #{classes.lookups()}"
