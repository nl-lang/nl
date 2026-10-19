# A call; a `(` on the same line after whitespace is still
# a call; `*expr` spreads as the last argument; a trailing comma.
f()
f(a, b)
f (x)
f(a, *rest)
f(a, b,)
f(1)(2)
