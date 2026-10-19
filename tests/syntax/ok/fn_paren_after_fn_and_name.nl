# After `fn` and after a declared name a `(` on the same line
# opens the parameter list, with or without whitespace; on the next line it
# starts the body.
a = fn (x) x end
b = fn
(x)
end
fn foo (x)
end
fn bar
(x)
end
