# cond runs the first branch whose condition is truthy
fn compare(a, b)
  cond
  when a > b
    "greater"
  when a < b
    "less"
  else
    "equal"
  end
end

print compare(2, 1)
print compare(1, 2)
print compare(1, 1)

let x = 5
cond
when x > 10
  print "big"
when x > 3
  print "medium"
when x > 1
  print "never: the first truthy branch wins"
end

cond
when nil
  print "nil is falsy"
when 0
  print "0 is truthy"
end
