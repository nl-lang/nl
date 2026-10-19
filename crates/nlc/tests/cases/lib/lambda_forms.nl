# Short functions: the same program as lambda_forms_fn.nl,
# written with `->`. The two compile to the same code (cases.rs,
# `lambda_same_code`).
fn apply(x: any, f: Function): any
  f(x)
end
let xs = [3, 1, 2]
let inc = x -> x + 1
let mul = |a: Number, b: Number|: Number -> a * b
let yes = || -> true
let pair = |a, b = 2| -> do return a, b end
let total = 0
for i in 1..3
  total += apply(i, v -> v * 10 + i)
end
:table.sort(xs, |a, b| -> a < b)
print inc(1), mul(2, 3), yes(), pair(1), total, xs[1]
