# A declared Lua class (gmod's Color) checks a parameter by its metatable, which FindMetaTable gives
fn brightness(c: Color): Number
  c.r + c.g + c.b
end

let total = 0
for i in 1..50
  total += brightness(:Color(i, 1, 1))
end
print total
print :Color(1, 2, 3) is Color
let fake: any = {r: 1, g: 2, b: 3}
print fake is Color
try
  brightness(fake)
catch e: TypeError
  print "TypeError: #{e.message}"
end
