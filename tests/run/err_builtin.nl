# Built-in exception types; every caught value is an Exception
let text: any = "text"
let num: any = 5
let nothing: any = nil

fn kind_of_error(f)
  try
    f()
    "no error"
  catch err
    typeof(err)
  end
end

fn type_error
  let n: Number = text
end

fn match_error
  let [a] = num
end

fn conversion_error
  text as Number
end

fn lua_error
  let v = nothing.field
end

fn lua_error_value
  error("plain lua error", 0)
end

fn fine
  1
end

print kind_of_error(type_error!)
print kind_of_error(match_error!)
print kind_of_error(conversion_error!)
print kind_of_error(lua_error!)
print kind_of_error(lua_error_value!)
print kind_of_error(fine!)

try
  error("plain lua error", 0)
catch err: LuaError
  print err.value
end

try
  error({code: 7})
catch err: LuaError
  print err.value.code
end

try
  let n: Number = text
catch err: Exception
  print(err is TypeError)
end

try
  let n: Number = text
catch err
  print(err.trace is String)
end
