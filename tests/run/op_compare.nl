# ==, <, <= definitions; != derived; > and >= fall back to swapped < and <=
struct Version
  n: Number
end

fn Version.==(right)
  @n == right.n
end

fn Version.<(right)
  @n < right.n
end

fn Version.<=(right)
  @n <= right.n
end

let v1 = Version { n: 1 }
let v2 = Version { n: 2 }
let v1b = Version { n: 1 }
print(v1 == v1b)
print(v1 != v1b)
print(v1 != v2)
print(v1 < v2)
print(v2 <= v1)
print(v2 > v1)
print(v1 >= v2)
print(v1 == 1)

struct Loud
  n: Number
end

fn Loud.<(right)
  print "Loud.<"
  @n < right.n
end

fn Loud.>(right)
  print "Loud.>"
  @n > right.n
end

let l1 = Loud { n: 1 }
let l2 = Loud { n: 2 }
print(l2 > l1)
print(l1 < l2)
