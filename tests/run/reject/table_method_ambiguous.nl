# Rejected by the analyzer.
fn total(v: [Number] | {[String]: Number}): Number
  v.values.sum
end

# expect-error: `reverse` is Table.reverse on the list and String.reverse on the string, and no one call fits both: narrow `v` with `is`
fn flip(v: [Number] | String) v.reverse end
print total([1]), flip("ab")
