-- Floor division and modulo: the digit sums of a range of numbers
-- (`n % 10`, `math.floor(n / 10)` in a while loop), and a mixed-sign
-- floor division and modulo.
local floor = math.floor
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 4000000

local sum = 0
for _ = 1, scale do
  for i = 1, n do
    local m = i
    while m > 0 do
      sum = sum + m % 10
      m = floor(m / 10)
    end
    sum = sum + floor((i - 500000) / 7) + (500000 - i) % 13
  end
end
print(sum)
