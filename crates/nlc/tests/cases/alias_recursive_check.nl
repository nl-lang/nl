# checked-only
# Finding 1: a recursive alias checks values all the way down, and the
# message names the inner reference by the alias's name.
type Node = {value: Number, next: Node?}
let bad: any = {value: 1, next: {value: 2, next: {value: "three"}}}
try
  let n: Node = bad
catch e
  print typeof(e), e.message
end
let good: any = {value: 1}
let m: Node = good
print m.value
