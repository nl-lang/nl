# coerce when a number is the left operand; ordering fallbacks reach coerce; == never does
struct Money
  cents: Number
end

fn Money.+(right)
  Money { cents: @cents + right.cents }
end

fn Money.*(right)
  Money { cents: @cents * right.cents }
end

fn Money.<(right)
  @cents < right.cents
end

fn Money.coerce(left)
  print "coerce #{left}"
  return Money { cents: left }, self
end

let m = Money { cents: 5 }
print((2 + m).cents)
print((3 * m).cents)
print(1 < m)
print(m > 4)
print(2 == m)

struct NoAdd
  v: Number
end

fn NoAdd.coerce(left)
  print "NoAdd.coerce"
  return NoAdd { v: left }, self
end

let na: any = NoAdd { v: 1 }
try
  let x = 1 + na
catch
  print "error after coerce"
end
