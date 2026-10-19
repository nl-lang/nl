# A Lua path that is its first name alone, or ends in a
# call, a `:name` or a block, is not an assignment target.
:x = 1
:f() = 1
:t:m = 1
:t.each do end = 1
