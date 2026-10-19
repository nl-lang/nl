# checked-only
# `*args: T` types the table of the remaining arguments, as TypeScript's
# `...args: T[]` does; `[Number]` is a table at runtime.
fn total(first: Number, *more: [Number]): Number
  let sum = first
  for v in more
    sum += v
  end
  sum
end
print total(1, 2, 3)
fn strict(first: Number, *more: Number)
  more
end
try
  strict(1, 2)
catch err
  print typeof(err), err.message
end
