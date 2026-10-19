# A word before `:` at the start of an item is a
# key whatever the spacing; a Lua path value needs the key's colon first.
t = {a :1, f :x, b : 2}
u = {f: :x, g: :a.b}
