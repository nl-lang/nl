# `do` at the end of a header: `while x do` is reported and the
# `do` dropped, so the `end` matches the header.
while x do
  y()
end
until x do
  y()
end
