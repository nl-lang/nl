# After a header that ends in an expression a line break or
# `;` is required, so a closer on the header's line is an error (`if x end`,
# `while x end`), and `f(if a 1 else 2 end)` cannot parse as `if (a 1)`.
if x end
while x end
f(if a 1 else 2 end)
