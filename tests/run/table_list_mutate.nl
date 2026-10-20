# Table's list changes: push, pop, shift, unshift, append, extend, insert, remove, clear, shuffle, sample
let xs = [1, 2]

print Table.push(xs, 3, 4) == xs, xs.join(",")
print Table.pop(xs), xs.join(",")
print Table.shift(xs), xs.join(",")
print Table.unshift(xs, -1, 0) == xs, xs.join(",")
print Table.pop([]), Table.shift([])

let a = [1, 2]
let b = [3]
let both = Table.append(a, b)
print both.join(","), a.join(","), both == a
print Table.extend(a, b) == a, a.join(",")

# Lua's own functions are there too
Table.insert(a, 1, 0)
Table.insert(a, 9)
print a.join(","), Table.remove(a, 1), Table.remove(a), a.join(",")
print Table.concat(a, "+"), Table.maxn([1, 2, nil, 4]), Table.getn([7, 8])

let m = {k: 1, j: 2}
print Table.clear(m) == m, Table.is_empty(m)

# shuffle and sample take a random source: rng(n) is an integer in 1..n
let picks = [3, 1, 2, 1]
let i = 0
let rng = fn(n)
  i += 1
  Number.min(picks[i], n)
end
let deck = ["a", "b", "c", "d"]
print Table.shuffle(deck, rng!).join(""), deck.join("")
print Table.sample(deck, n -> n), Table.sample([], n -> 1)

# with math.random, seeded: a permutation of the same values
Number.randomseed(7)
let shuffled = Table.shuffle([1, 2, 3, 4, 5, 6])
print Table.sorted(shuffled).join(","), Table.includes(deck, Table.sample(deck))
