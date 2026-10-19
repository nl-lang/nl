# `//` never yields negative zero. `floor(0 / -9)` is -0.0
# in LuaJIT's interpreter and 0 once the JIT has compiled it, so without
# the canonical zero one run printed both. The loop makes `div` hot.
# backends: lua,bc
fn div(a, b)
  a // b
end

fn typed(a: Number, b: Number): Number
  a // b
end

print(tostring(0 // -9), tostring(-0.0 // 5), tostring(typed(0, -9)))
let x = 0
x //= -3
print(tostring(x))
let seen = []
for i in 1..200
  let s = tostring(div(0, -9)) <> " " <> tostring(typed(0, -9))
  Table.insert(seen, s) if i % 50 == 0
end
print(Table.concat(seen, ","))
print(7 // -2, -7 // 2, 7.5 // 2)
