# `try`/`catch` in a hot loop where nothing raises: a protected call of a
# small function that could fail, its value used.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 10000000

fn parse_digit(c)
  raise "not a digit" if c < 0 or c > 9
  c * 3 + 1
end

let check = 0
let failures = 0
for _rep in 1..scale
  for i in 1..n
    let v = try
      parse_digit(i % 10)
    catch e
      failures += 1
      0
    end
    check += v
  end
end
print check, failures
