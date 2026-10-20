# Rejected by the analyzer.
fn String.shout
  self.upcase <> "!"
end

# expect-error: `wrap` is a function of String's library, which a file cannot redefine
fn String.wrap(width: Number)
  [self]
end
