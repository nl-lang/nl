# Rejected by the analyzer.
fn Table.second
  self[2]
end

# expect-error: `map` is a function of the Table library, which a file cannot redefine
fn Table.map(f: Function)
  self
end
