# Every step of a Lua path has a nil-safe form; `?.` goes into the step's node.
x = :LocalPlayer()?.:GetPos()
x = :a?.b?.[1]?.(2)?.:m(3)
x = :a?.b
  ?.c
:cfg?.max = 8
:cfg?.list[1] = 0
