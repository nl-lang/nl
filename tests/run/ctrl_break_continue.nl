# break and continue in every loop form
let odds = []
for i in 1..10
  continue if i % 2 == 0
  break if i > 7
  Table.insert(odds, i)
end
print Table.concat(odds, ",")

let j = 0
let seen = []
while j < 6
  j += 1
  continue if j == 3
  Table.insert(seen, j)
end
print Table.concat(seen, ",")

let combos = []
for a in 1..3
  for b in 1..3
    break if b > a
    Table.insert(combos, "#{a}#{b}")
  end
end
print Table.concat(combos, " ")

let k = 0
loop
  k += 1
  continue if k < 5
  break
end
print k
