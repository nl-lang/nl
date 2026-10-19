# `catch name: Type`, `catch name`, a bare `catch`, a union in
# the type, `finally`.
try
  risky()
catch err: NotFound
  a()
catch e: NotFound | Timeout
  b()
catch err
  c()
catch
  d()
finally
  e()
end
