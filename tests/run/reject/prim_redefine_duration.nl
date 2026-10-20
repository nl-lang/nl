# Rejected by the analyzer.
fn Number.fortnights
  (self * 2).weeks
end

# expect-error: `minutes` is one of Number's Duration helpers, so it is already defined
fn Number.minutes
  self * 60
end
