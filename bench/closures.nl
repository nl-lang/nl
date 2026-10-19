# Closure creation and calls: a closure made per iteration (capturing a
# loop variable), called a few times, and a block passed to a function
# that takes a Function parameter.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 8000000

fn make_adder(k)
  fn(x) x + k end
end

fn apply_twice(x: any, f: Function): any
  f(f(x))
end

let check = 0
for _rep in 1..scale
  for i in 1..n
    let add = make_adder(i % 100)
    check += add(1) + add(2)
    check += apply_twice(i % 10) do |v| v * 2 + 1 end
    check = check % 1000003
  end
end
print check
