-- List access by index: a sieve of Eratosthenes (writes), then prefix
-- sums over a list of numbers (reads and writes by index, `#`).
local floor = math.floor
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 2000000

local check = 0
for _ = 1, 16 * scale do
  local composite = {}
  for i = 1, n do
    composite[i] = false
  end
  local count = 0
  for i = 2, n do
    if not composite[i] then
      count = count + 1
      local j = i * i
      while j <= n do
        composite[j] = true
        j = j + i
      end
    end
  end
  local a = {}
  for i = 1, n do
    a[i] = i % 17
  end
  for i = 2, #a do
    a[i] = a[i] + a[i - 1]
  end
  check = check + count + a[n] + a[floor(n / 2)]
end
print(check)
