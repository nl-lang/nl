# A value that cannot be a key (nil, NaN) of what group_by, tally, invert,
# to_set, map_keys and map_kv build raises a TypeError naming the function
let nan = 0 / 0
fn attempt(label, f)
  try
    f()
    print "#{label}: no error"
  catch err: TypeError
    print "#{label}: #{err.message}"
  end
end

attempt "group_by nil", || -> Table.group_by([1, 2], x -> nil)
attempt "group_by NaN", || -> Table.group_by([1, 2], x -> x * nan)
attempt "tally", || -> Table.tally([1, nan])
attempt "invert", || -> Table.invert({a: nan})
attempt "to_set", || -> Table.to_set([nan])
attempt "map_keys", || -> Table.map_keys({a: 1}, |k, v| -> nan)
fn nan_key(k, v)
  return nan, v
end
attempt "map_kv", || -> Table.map_kv({a: 1}, nan_key!)

# ordinary keys still work, and a nil key from map_keys drops the pair
print Table.sorted_keys(Table.group_by([1, 2, 3], x -> x % 2)).join(","), Table.tally(["a", "a"]).a
print Table.size(Table.map_keys({a: 1, b: 2}, |k, v| -> k == "a" and k or nil))
