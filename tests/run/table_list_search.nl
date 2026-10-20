# Table's list queries: find, index_of, includes, any/all/none/one, count, min/max, sum, tally, group_by, partition
let xs = [4, 8, 15, 16, 23, 42]

print Table.find(xs, x -> x > 10), Table.find(xs, x -> x > 100)
print Table.find_index(xs, x -> x % 2 == 1), Table.find_index(xs, 16), Table.find_index(xs, 5)
print Table.index_of([1, 2, 1, 2], 2), Table.index_of([1, 2, 1, 2], 2, 3), Table.index_of(xs, 0)
print Table.includes(xs, 23), Table.include(xs, 24)

# without a predicate the values themselves are tested
print Table.any(xs, x -> x > 40), Table.any([false, nil]), Table.any([])
print Table.all(xs, x -> x > 0), Table.all([1, false]), Table.all([])
print Table.none(xs, x -> x > 50), Table.none([false]), Table.none([1])
print Table.one(xs, x -> x > 40), Table.one(xs, x -> x > 20), Table.one([false, 3])

print Table.count(xs), Table.count(xs, x -> x < 16), Table.count([1, 2, 1, 1], 1)

print Table.min(xs), Table.max(xs), Table.min([]), Table.max(["b", "c", "a"])
let longest = |a, b| -> a.len > b.len
print Table.min(["ccc", "a", "bb"], longest!)
let words = ["kiwi", "fig", "banana", "plum"]
print Table.min_by(words, w -> w.len), Table.max_by(words, w -> w.len), Table.min_by([], w -> w)

print Table.sum(xs), Table.sum([]), Table.sum(words, w -> w.len)
print Table.sum([1, 2, 3], |x, i| -> x * i)

let counts = Table.tally(["a", "b", "a", "c", "a", "b"])
Table.each_sorted(counts, |k, n| -> print("#{k}: #{n}"))

let groups = Table.group_by(words, w -> w.len)
Table.each_sorted(groups, |n: Number, ws: [String]| -> print(n, ws.join(",")))

let evens, odds = Table.partition(xs, x -> x % 2 == 0)
print evens.join(","), odds.join(",")
