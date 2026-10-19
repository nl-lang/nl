# A Lua path that ends in a call, an index or a block is
# not a command callee; a `{` after it is not a table argument.
:f() a
:t[1] b
:t:m() c
:f() {a: 1}
