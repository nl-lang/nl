# `{` after a capitalized name path opens a struct literal; after
# a lowercase callee it starts a table argument of a command call.
x = Foo {a: 1}
x = Lib.Foo {a: 1}
f {a: 1}
x.m {a: 1}
x.on {a: 1}
