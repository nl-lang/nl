# A nil-safe target needs an assignment of its own.
a?.b, c = 1, 2
c, :a?.b = 1, 2
