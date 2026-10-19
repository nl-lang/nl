# Recovery (indentation): the inner `if` lacks its `end`; the `else` at the
# outer `if`'s indentation belongs to the outer `if`.
if a
  if b
    c
else
  d
end
