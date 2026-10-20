# `then` in the headers of `case`, `catch` and `for`, each reported
# once; a `when` takes `then`, so the one after `case x then` parses.
case x then
when 1 then foo()
end
try
  f()
catch e then
  g()
end
for i in list then
  h(i)
end
