# `while x do y end` passes a block to `x`, so the `while`
# has no `end`: one targeted error at the `do`.
while x do y end
z = 1
