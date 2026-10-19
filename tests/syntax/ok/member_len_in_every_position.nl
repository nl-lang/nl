# `.len` parses as an ordinary member in every position; the
# resolver rejects its misuse.
n = x.len
x.len()
x.len a
f = x.len!
x.len do end
x.len = v
x.len += v
y = @len
k = t["len"]
m = :t.len
fn Foo.len
end
