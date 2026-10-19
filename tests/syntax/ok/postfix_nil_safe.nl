# The nil-safe steps `?.name`, `?.[k]`, `?.(args)` and `?.:m(args)`; the `?.` sits in the step's own node.
x = a?.b
x = a?.len
x = a?.end
x = a?.[k]
x = f?.(1, *rest, k: 2)
x = f?.() do |v| v end
x = o?.:m(1)
x = o?.:sendMessage
x = a?.b.c(d)?.e
x = a ?. b
x = a?.f!
x = (a?.b).c
