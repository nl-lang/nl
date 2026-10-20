# Rejected by the analyzer.
fn Table.second
  self[2]
end

print Table.second([1, 2])

# expect-error: called through `Table`, the function takes its receiver as the first argument, which must be a table
print Table.second("ab")
