# Every misuse of `.len` parses (`x.len = v`, `x.len += v`,
# `x.len!`, `x.len()`, `x.len a`, `x.len do … end`, `@len`, a field `len`);
# the resolver reports them.
x.len = v
x.len += v
f = x.len!
x.len()
x.len a
x.len do end
y = @len
struct S
  len: Number
end
