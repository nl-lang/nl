# `then` in the headers of `when` (of `case` and `cond`),
# `catch`, `for` and `case`.
case x then
when 1 then foo()
end
cond
when a then
  b()
end
try
  f()
catch e then
  g()
end
for i in list then
  h(i)
end
