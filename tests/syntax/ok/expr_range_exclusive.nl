# `a...b` is the range that leaves out its end, at the
# level of `..`; with numbers, names, calls and negative bounds, in an index,
# a `for` and a `when`.
a = 1...10
b = lo...hi
c = f(x)...n.len
d = -3...-1
e = s[2...4]
for i in 0...n
end
case v
when 1...5
  low()
end
