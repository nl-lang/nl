# while, until, loop, and the while/until modifiers
let i = 0
while i < 3
  i += 1
end
print i

until i == 0
  i -= 1
end
print i

let n = 0
loop
  n += 1
  break if n >= 4
end
print n

let x = 0
x += 1 while x < 5
print x
x -= 2 until x < 0
print x
let y = 0
y += 1 while false
print y
