# Ranges as values and in numeric loops
let total = 0
for i in 1..4
  total += i
end
print total

let excl = 0
for i in 1...4
  excl += i
end
print excl

let r = 2..5
let from_value = 0
for i in r
  from_value += i
end
print from_value

let n = 3
let upto = []
for i in 1..n + 1
  Table.insert(upto, i)
end
print Table.concat(upto, ",")

let odd = []
for i in 1..9 step 2
  Table.insert(odd, i)
end
print Table.concat(odd, ",")

let none = 0
for i in 1...1
  none += 1
end
print none

let half_open = 0...n
let count = 0
for i in half_open
  count += 1
end
print count
