# Table's map functions: keys, values, entries, each_kv, map_values, map_keys, map_kv, filter_kv, reject_kv, invert, to_set
let stock = {apple: 3, pear: 0, fig: 12}

print Table.sorted(Table.keys(stock)).join(",")
print Table.sorted(Table.values(stock)).join(",")
let rows = Table.sort_by(Table.entries(stock), e -> e[1])
print Table.map(rows, e -> "#{e[1]}=#{e[2]}").join(" ")
print Table.keys({}).len, Table.entries({}).len

# callbacks get the key, then the value
let total = 0
print Table.each_kv(stock, |_, n| -> do total += n end) == stock, total

let show = m -> Table.map(Table.sorted_keys(m), k -> "#{k}:#{m[k]}").join(" ")
print show(Table.map_values(stock, |k, n| -> n * 2))
print show(Table.map_keys(stock, |k, n| -> k.upcase))
print show(Table.map_kv(stock, |k, n| -> do return "#{k}s", n + 1 end))
print show(Table.filter_kv(stock, |_, n| -> n > 0))
print show(Table.reject_kv(stock, |k, _| -> k.len > 3))
print show(stock)

let codes = Table.invert({a: 1, b: 2})
print codes[1], codes[2]
let seen = Table.to_set(["x", "y", "x"])
print show(seen), seen.x, seen.z

Table.each_sorted(stock, |k, n| -> print(k, n))
print Table.sorted_keys(stock, |a, b| -> a.len < b.len).join(",")
