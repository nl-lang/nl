# `fn f() :Number` is a body that starts with a
# Lua path; the compiler warns. The same after block parameters.
fn f() :Number
end
g do |a| :print(a) end
