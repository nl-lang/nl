# `when P if cond` adds a guard; the guard may be a command
# call.
case p
when {hp} if hp > 0
  alive()
when {x} if near x
  close()
end
