# A nil-safe target in an assignment of its own, simple or compound.
a?.b = 1
a?.[k] = 2
a?.b.c = 3
a?.b += 1
a?.b ??= 0
:p?.x = 4
