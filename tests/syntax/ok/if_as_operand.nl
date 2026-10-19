# `if` begins an expression where an operand is
# expected: after `=`, inside parentheses, after an operator.
label = if a
  "a"
else
  "b"
end
f(if a; 1 else 2 end)
