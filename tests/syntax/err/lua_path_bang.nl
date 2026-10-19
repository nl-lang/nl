# Postfix `!` after any step of a Lua path is an error;
# the path goes on after it.
a = :f!
b = :f().g!
c = :a.b!.c
