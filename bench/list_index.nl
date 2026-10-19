# List access by index: a sieve of Eratosthenes (writes), then prefix
# sums over a list of numbers (reads and writes by index, `.len`).
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 2000000

let check = 0
for _rep in 1..16 * scale
  let composite = []
  for i in 1..n
    composite[i] = false
  end
  let count = 0
  for i in 2..n
    unless composite[i]
      count += 1
      let j = i * i
      while j <= n
        composite[j] = true
        j += i
      end
    end
  end
  let a = []
  for i in 1..n
    a[i] = i % 17
  end
  for i in 2..a.len
    a[i] = a[i] + a[i - 1]
  end
  check += count + a[n] + a[n // 2]
end
print check
