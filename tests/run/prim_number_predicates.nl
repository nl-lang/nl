# Number predicates: even, odd, divisible_by, is_integer, is_nan, is_finite
let nan = 0 / 0
let inf = 1 / 0
print 4.even, 3.even, 0.even, (-2).even, 2.5.even
print 3.odd, 4.odd, (-3).odd, 2.5.odd, inf.odd
print 100.divisible_by(10), 100.divisible_by(7), (-9).divisible_by(3), 7.5.divisible_by(2.5), 1.divisible_by(0)
print 3.is_integer, 3.5.is_integer, (-0).is_integer, inf.is_integer, nan.is_integer
print nan.is_nan, 1.is_nan, inf.is_nan
print 3.is_finite, inf.is_finite, (-inf).is_finite, nan.is_finite

# Predicates as filters
let evens = []
for n in 1..10
  Table.insert(evens, n) if n.even && !n.divisible_by(4)
end
print Table.concat(evens, " ")
