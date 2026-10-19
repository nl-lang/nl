# NL and Lua call each other; NL exceptions are ordinary Lua error values
let helper = require("lua.helper")

fn square(x)
  x * x
end

print helper.apply(square!, 4)

struct Box
  v: Number
end

fn Box.scaled(n)
  @v * n
end

print helper.call_method(Box { v: 3 }, 5)

fn raises
  raise "raised in NL"
end

let ok, err = helper.protect(raises!)
print ok
print err.message
print typeof(err)

try
  helper.fail()
catch e: LuaError
  print e.value.code
end

try
  helper.fail_plain()
catch e: LuaError
  print e.value
end

let pcall_ok, pcall_err = pcall(raises!)
print pcall_ok
print pcall_err.message
