# Integer arithmetic: + - * and comparisons on whole numbers (doubles that
# hold integers), in a nested numeric loop with a running checksum kept
# below 2^31.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 10000

let sum = 0
for _rep in 1..scale
  for a in 1..n
    let a2 = a * a
    for b in a..n
      let c2 = a2 + b * b - a * b
      if c2 > sum
        sum = sum + c2 - 3 * b
      else
        sum = sum - c2 + 7 * a
      end
      sum -= 1000000000 if sum > 2000000000
    end
  end
end
print sum
