# A doc comment of `##` lines attaches to the declaration
# on the line directly after it (fn, struct field, enum variant, let).
## Adds two numbers.
## Second line.
fn add(a, b)
  a + b
end

struct S
  ## The id.
  id: Number
end

enum E
  ## The first.
  A
end

## A variable.
let v = 1
