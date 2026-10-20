# Rejected by the resolver: Nest.Inner is a type nested in a declared module.
fn Shelf.size
  Shelf.count()
end

# expect-error: a nested declared type has no registered name and no Lua path, so no known metatable
fn Nest.Inner.check
  true
end
