# The call form with a call_path callee, postfix after the
# call, the command form, the block form.
t = spawn find_path(from, to)
t = spawn shape.area()
t = spawn Nav.find(a, b)
t = spawn self.step(x)
t = spawn @worker(1)
r = spawn find_path(from, to).await
spawn f a, b
t = spawn do String.rep(s, n) end
r = spawn do work() end.await
