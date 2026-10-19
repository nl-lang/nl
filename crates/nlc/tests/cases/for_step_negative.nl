# A negative step counts down, in inclusive and exclusive ranges, with a
# literal and with a computed step; zero is an error.
for i in 5..1 step -2
  print i
end
for i in 5...1 step -2
  print i
end
let s: any = -1
for i in 3..1 step s
  print i
end
for i in 3...1 step s
  print i
end
for i in 1...4 step s * -1
  print i
end
try
  for i in 1..3 step 0
    print i
  end
catch err
  print "zero step: #{typeof(err)}"
end
