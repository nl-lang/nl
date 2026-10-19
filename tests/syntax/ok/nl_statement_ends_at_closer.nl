# A statement may end at a closing `end`, `else`, `)` on the same
# line.
if a
  b() else c() end
f(fn() x end)
g(fn() return end, 1)
