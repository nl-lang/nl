# Operators defined on Number and String; Number.* runs in preference to coerce
struct Meters
  v: Number
end

fn Meters.coerce(left)
  print "Meters.coerce"
  return Meters { v: left }, self
end

fn Meters.+(right)
  Meters { v: @v + right.v }
end

fn Number.*(right)
  print "Number.*"
  right.v * self
end

let m = Meters { v: 4 }
print(2 * m)
print((2 + m).v)
print(2 * 3)

fn String.*(n)
  self.rep(n)
end

print("ab" * 3)
