# Duration operators: + - * / and unary -, comparisons, ==, abs
let a = 90.minutes
let b = 30.minutes
print a + b, a - b, b - a, -a
print a * 2, 2 * a, a / 3, a / b, b / a
print typeof(a * 2), typeof(a / b)
print a > b, a < b, a >= 90.minutes, a <= b, a == 5400.seconds, a != b
print((b - a).abs, a.abs == a)
let total = 0.seconds
for n in [1, 2, 3]
  total += n.minutes
end
print total
case 2.hours
when 120.minutes
  print "two hours"
else
  print "something else"
end
let five: any = 5
let word: any = "x"
for f in [|| -> a + five, || -> five - a, || -> a * a, || -> five / a, || -> a < five, || -> word * a]
  try
    f()
  catch err: TypeError
    print err.message
  end
end
