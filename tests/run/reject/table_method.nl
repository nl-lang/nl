# Rejected by the compiler.
# expect-error: `Table` cannot have methods; plain tables have no shared metatable
fn Table.first
  self[1]
end
