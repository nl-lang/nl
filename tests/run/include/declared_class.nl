# Helper for type_declared_class.nl: a function whose parameter is a declared Lua class.
pub fn x_of(v: Vec): Number
  v.x
end

pub fn attempt(v: any): String
  try
    "x = #{x_of(v)}"
  catch e: TypeError
    e.message
  end
end
