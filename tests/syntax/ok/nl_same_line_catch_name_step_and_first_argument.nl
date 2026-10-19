# The name of `catch`, the `step` of a `for` header and the first
# argument of a command call are only taken on the same line; on the next line
# they begin the body or a new statement.
try
  f()
catch
  e
end
for i in 1..10
  step 2
end
print
"x"
