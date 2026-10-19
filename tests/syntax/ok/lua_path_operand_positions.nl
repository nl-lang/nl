# A `:` touching the word after it is a Lua path wherever
# an operand may start.
f(:a.b)
x = -:a.b
x = !:cfg.on
x = 1..:n
x = {f: :x}
x = a + :b
return_value = :x if :y
