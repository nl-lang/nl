# backends: lua,bc
# The positional arguments of a call are evaluated before its keyed ones,
# also when a keyed value needs statements of its own.
fn g
  print "g"
  1
end
fn h
  print "h"
  nil
end
fn f(a, **opts)
  print "f #{a} #{opts.k}"
end
f(g(), k: h() ?? 2)
f(g(), **(h() ?? {k: 3}))
f(g(), k: (if h() then 4 else 5 end))
