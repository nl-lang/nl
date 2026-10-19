# Keywords are plain names after `.` and as table keys; `from` and `step` are contextual
let t = {type: "fruit", end: 3, if: true}
print t.type
print t.end
print t.if
t.end = 4
print t.end
let from = 1
let step = 2
print from + step
for i in 1..5 step 2
  print i
end
