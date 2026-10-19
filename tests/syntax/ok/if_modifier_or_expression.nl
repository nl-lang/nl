# After a complete expression `if` is a modifier; where an
# operand is expected it begins an expression; after `return` and at the
# start of a command argument it is a modifier.
f() if x
y = if x; 1 else 2 end
print if x
fn g
  return if done
  return (if a; b else c end)
end
