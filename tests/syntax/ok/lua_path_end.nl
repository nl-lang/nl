# A Lua path takes every step that follows it: `.name`,
# `[expr]`, calls, `:name`; after a call, an index or a method the chain is
# still Lua. Parentheses end the path, and a `.name` after them is NL.
a = :x.getA.getB
b = :x.y().z
c = :x.y[1].z
d = :x.y:z().w
e = (:x.y()).z
