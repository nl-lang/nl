# A `(` on the same line after a postfix expression is a call,
# with or without whitespace; at the start of a line it begins a statement.
f (x)
y = f (a + b) * 2
g
(z)
