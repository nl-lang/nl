# A modifier may follow an expression statement, an
# assignment, a compound and a multiple assignment, `return`, `break`,
# `continue`.
self.die if @health == 0
x = 1 if a
x += 1 if a
x, y = y, x if a
loop
  break if done
  continue unless ready
end
