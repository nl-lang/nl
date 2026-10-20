# Rejected by the analyzer.
fn Number.is_cool
  self == 7
end

# expect-error: `round` is a function of Number's library, which a file cannot redefine
fn Number.round
  self
end
