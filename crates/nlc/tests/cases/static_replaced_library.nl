# The direct code of `//` and the bitwise
# operators calls the library functions the runtime captured when it
# opened, so a script that replaces `math.floor` or `bit.band` changes
# neither typed nor untyped operators.
# backends: lua,bc
:math.floor = fn(x) 99 end
:bit.band = fn(x, y) 99 end
let a: Number = 7
let b = tonumber("7")
print(a // 2, b // 2, a & 3, b & 3)
