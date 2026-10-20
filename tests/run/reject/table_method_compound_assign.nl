# Rejected by the analyzer.
let totals: {[String]: Number} = {}
totals["sum"] = 1
totals.apples = 2
totals.apples += 1

# expect-error: a compound assignment follows the same rule as `=`: `sum` is Table.sum on a map
totals.sum += 1
