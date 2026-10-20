# Rejected by the analyzer.
let counters: Table = {}
counters["count"] = 1
print counters.count

# expect-error: on a Table, `count` is Table.count when read, so it cannot be stored as a key with `.`
counters.count = 2
