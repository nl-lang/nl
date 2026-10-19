# `fn (` on the same line opens the parameter list with or
# without whitespace; on the next line it starts the body.
a = fn (x) x end
b = fn
(x)
end
c = fn() (a or b).x end
