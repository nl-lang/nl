# Rejected by the analyzer.
fn String.shout
  self.upcase <> "!"
end

# expect-error: called through its type, the method takes its receiver as the first argument, which must be a String
print String.shout(5)
