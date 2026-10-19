# A `[` on the same line after a postfix expression is an index;
# in a pattern it opens a positional pattern, in a type an array type, at the
# start of an operand a list.
x = f [1]
let [a] = t
let y: [Number] = t
z = [k, 1]
w = f([1, 2])
