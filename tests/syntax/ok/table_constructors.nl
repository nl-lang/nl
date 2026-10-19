# Lists in brackets, maps in braces: word
# keys, number keys, string keys, nested tables, trailing commas, spreads,
# the empty list and the empty map.
a = [1, 2, 3]
b = { a: 1, b: 2, 1: 3, }
c = { "any string": 1, type: 2 }
d = [[1], {x: {y: 2}}, [*a, 4], [*a, *a]]
e = {}
f = []
g = [1, nil, 3,]
h = f([1, 2])
i = f a, [1, 2]
[1, 2].len
