# `fn`, `spawn`, `on`, `case`, `cond` and `try` start a
# command argument.
f fn(x) x end
f spawn g(1)
f on Hit do end
f case x
  when 1
    g()
end
f cond
  when a
    b()
end
f try
  g()
catch
end
