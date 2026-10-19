# Short functions: the same program as lambda_forms.nl,
# written with `fn`. The two compile to the same code (cases.rs,
# `lambda_same_code`).
fn apply(x: any, f: Function): any
  f(x)
end
let xs = [3, 1, 2]
let inc = fn(x) x + 1 end
let mul = fn(a: Number, b: Number): Number a * b end
let yes = fn() true end
let pair = fn(a, b = 2) return a, b end
let total = 0
for i in 1..3
  total += apply(i, fn(v) v * 10 + i end)
end
:table.sort(xs, fn(a, b) a < b end)
print inc(1), mul(2, 3), yes(), pair(1), total, xs[1]
