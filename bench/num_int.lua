-- Integer arithmetic: + - * and comparisons on whole numbers (doubles that
-- hold integers), in a nested numeric loop with a running checksum kept
-- below 2^31.
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 10000

local sum = 0
for _ = 1, scale do
  for a = 1, n do
    local a2 = a * a
    for b = a, n do
      local c2 = a2 + b * b - a * b
      if c2 > sum then
        sum = sum + c2 - 3 * b
      else
        sum = sum - c2 + 7 * a
      end
      if sum > 2000000000 then sum = sum - 1000000000 end
    end
  end
end
print(sum)
