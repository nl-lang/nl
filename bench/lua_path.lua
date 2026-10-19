-- Library functions in a hot loop, as a Lua programmer writes them: a local
-- alias of each (`local floor, sin = math.floor, math.sin`).
local floor, sin = math.floor, math.sin
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 10000000

local function run(count)
  local total = 0
  for i = 1, count do
    total = total + floor(sin(i) * 1000) + floor(i / 3)
  end
  return total
end

local total = 0
for _ = 1, scale do
  total = total + run(n)
end
print(total)
