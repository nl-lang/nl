# A command call may be a header expression and the subject of
# `case`.
while f x
  g()
end
until f x
  g()
end
unless f x
  g()
end
case f x
when 1
  g()
end
cond
when f x
  g()
end
if let y = f x
  g()
end
