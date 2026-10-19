# Bracket patterns: names are positional; `*rest` or `*` last.
let [a, b] = pair
let [head, *tail] = list
let [first, *] = list
let [*] = list
let [x, 456,] = t
