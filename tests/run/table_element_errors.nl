# Table's arithmetic and comparing functions name themselves and the element they cannot use
let word: any = "a"
let mixed = [3, word, 1]
let cases = [
  || -> Table.sum(mixed),
  || -> Table.sum([1, 2], |v, _| -> if v == 2 then word else v end),
  || -> Table.min(mixed),
  || -> Table.max([word, 2]),
  || -> Table.sorted(mixed),
  || -> Table.sorted([{}, {}]),
  || -> Table.sorted([1.minutes, Time.at(0)]),
  || -> Table.min_by([1, 2], |v, _| -> if v == 2 then word else v end),
  || -> Table.max_by(["x", "y"], |v, _| -> if v == "y" then 1 else v end),
  || -> Table.sort_by([1, 2, 3], |v, _| -> if v == 3 then word else v end),
]
for f in cases
  try
    f()
    print "not reached"
  catch err: TypeError
    print err.message
  end
end
# what < takes still works: numbers, strings, values with the same __lt
print Table.sum([1, 2.5]), Table.sum([]), Table.sum(["ab", "c"], |s, _| -> s.len)
print Table.min([3, 1, 2]), Table.max(["b", "c", "a"]), Table.min([7]), Table.min([])
print Table.sorted([3.minutes, 1.minutes, 2.minutes]).join(" "), Table.max([Time.at(5), Time.at(9)]).epoch
print Table.sort_by(["ccc", "a", "bb"], |s, _| -> s.len).join(" "), Table.max_by([1, 5, 3], |v, _| -> -v)
print Table.min([1, word], |a, b| -> "#{a}" < "#{b}")
