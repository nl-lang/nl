# Floor division and modulo: the digit sums of a range of numbers
# (`n % 10`, `n // 10` in a while loop), and a mixed-sign `//` and `%`.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 4000000

let sum = 0
for _rep in 1..scale
  for i in 1..n
    let m = i
    while m > 0
      sum += m % 10
      m = m // 10
    end
    sum += (i - 500000) // 7 + (500000 - i) % 13
  end
end
print sum
