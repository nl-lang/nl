# Rejected by the analyzer.
let xs: [Number] = [1, 2, 3]
print xs.take(2).len

# expect-error: a method of a typed list is checked as its Table function: `take` wants a Number
print xs.take("two").len
