# A child struct uses the nearest operator definition among its ancestors, wherever it is declared
struct Money
  cents: Number
end

struct Price : Money
  label: String = "price"
end

fn Money.+(right)
  Money { cents: @cents + right.cents }
end

fn Money.-
  Money { cents: -@cents }
end

fn Money.<(right)
  @cents < right.cents
end

fn Money.==(right)
  @cents == right.cents
end

fn Money.[](key)
  "no #{key}"
end

fn Money.len
  @cents
end

fn Money.coerce(left)
  return Money { cents: left }, self
end

struct Tip : Price
  percent: Number = 0
end

let m = Money { cents: 5 }
let p = Price { cents: 7 }
let t = Tip { cents: 9 }

print typeof(p + m), (p + m).cents
print typeof(t + p), (t + p).cents
print((-t).cents)
print p < t, t < p, t > p
print p == Price { cents: 7 }, t == p, p != m
print p.missing, t.missing, t.label
print p.len, t.len
print((1 + t).cents)

fn Price.+(right)
  Price { cents: @cents + right.cents + 100 }
end

fn Price.len
  @cents * 2
end

print typeof(m + p), (m + p).cents
print typeof(p + m), (p + m).cents
print typeof(t + m), (t + m).cents
print m.len, p.len, t.len

fn total(list)
  let sum = Money { cents: 0 }
  for v in list
    sum = v + sum
  end
  sum
end
print total([m, m, m]).cents, total([t, p, m]).cents

struct Bare : Money
end
fn Tip.*(factor)
  Tip { cents: @cents * factor }
end
let b: any = Bare { cents: 1 }
print((b + b).cents)
try
  print b * 2
catch err
  print typeof(err)
end
print((t * 3).cents)
