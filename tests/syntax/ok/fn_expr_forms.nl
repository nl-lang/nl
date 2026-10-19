# Anonymous functions: with and without parameters, a return
# type, a body on the same line, an empty body, async.
a = fn(x) x * 2 end
b = fn print("hi") end
c = fn(a: Number): nil print('hello world') end
d = fn end
e = async fn(id) load(id) end
g = fn: Number 1 end
