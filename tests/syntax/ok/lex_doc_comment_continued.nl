# The `#` lines directly after a `##` line belong to the
# doc comment; a `#` line before the first `##` stays outside the declaration.
# plain
## aaa
# bbb
# ccc
fn foo; end

struct S
  ## The id.
  # More about the id.
  id: Number
end
