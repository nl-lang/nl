# Number rounding: round and truncate (with places), sign, clamp, between
print 2.5.round, (-2.5).round, 2.4.round, 0.5.round, 0.49999999999999994.round
print 2.675.round(2), 1.005.round(2), 1234.5678.round(2), (-1234.5678).round(1)
print 1250.round(-2), 1249.round(-2), (-1250).round(-2)
print 3.round(5), 1e300.round(2), (1 / 0).round

print 2.7.truncate, (-2.7).truncate, 2.675.truncate(2), 0.29.truncate(2)
print 1299.truncate(-2), (-1299).truncate(-2), (-1.239).truncate(2)

print 5.sign, (-3.5).sign, 0.sign

print 5.clamp(1, 3), 0.clamp(1, 3), 2.clamp(1, 3), 2.5.clamp(2.5, 2.5)
try
  5.clamp(3, 1)
catch err
  print "min > max: #{typeof(err)}"
end

print 3.between(1, 3), 1.between(1, 3), 4.between(1, 3), 2.5.between(2, 3)

# The same functions through the type, the receiver first
print Number.round(2.5), Number.clamp(9, 0, 5)

try
  let places: any = 1.5
  1.round(places)
catch err: TypeError
  print "places 1.5: TypeError"
end
