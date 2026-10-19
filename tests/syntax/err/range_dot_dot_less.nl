# `..<` is not an operator; the range that leaves out its
# end is `a...b`. Reported in an expression and in a pattern, and parsed as a
# range.
for i in 1..<10
end
let 1..<5 = q
x = 1
