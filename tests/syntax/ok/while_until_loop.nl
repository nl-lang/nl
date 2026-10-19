# `while`, `until` and `loop`; the body of `loop` may start on
# its line.
while running
  tick()
end
until done
  tick()
end
loop tick() end
