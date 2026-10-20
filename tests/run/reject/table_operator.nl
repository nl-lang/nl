# Rejected by the resolver.
fn Table.total
  self.sum
end

# expect-error: operators cannot be defined on `Table`; plain tables share no metatable
fn Table.+(other)
  self.append(other)
end
