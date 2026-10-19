# A `!` touching a keyword used as a name is not the start
# of a command argument; it is the postfix `!`, an error after a Lua path or a
# method call. The word after it is the argument.
:type!x
obj:end!y
