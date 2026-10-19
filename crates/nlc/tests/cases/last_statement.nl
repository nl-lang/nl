# Whether an expression statement's value is discarded is the
# resolver's decision, and the lowering follows it. Discarded
# means "a statement that is not last"; the last statement of a loop body,
# a `: nil` function, a handler and the file is last, so a variable there
# is read, not called, while a function-kind name or a method is called.
let calls = 0
fn count
  calls += 1
end
let t = {n: 5}
for i in 1..2
  t.n
end
for i in 1..2
  count
end
print calls
fn f: nil
  let v = 5
  v
end
f()
fn g: nil
  count
end
g()
print calls
on LastTest do |x|
  x
end
Event.run "LastTest", 1
count
print calls
let v = 5
v
