# After a keyword that does not follow `.`, `!` is prefix:
# `if!done` is `if !done`, `return!ok` is `return !ok`.
fn f
  if!done
    return!ok
  end
end
