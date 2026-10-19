# `if`, `unless`, `case`, `cond` and `try` as operands.
x = 1 + if a; 2 else 3 end
y = case n
when 1
  "one"
end
z = try
  f()
catch
  nil
end
w = unless a; 1 end
v = cond
when a
  1
end
