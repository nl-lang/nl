# Rejected by the resolver.
fn area(r)
  r * r * PI # a function may use a const declared below it
end
print area(1)
# expect-error: the file's own code reads `LIMIT` before the `const` line that gives it its value
print LIMIT
const LIMIT = 10
const PI = 3
