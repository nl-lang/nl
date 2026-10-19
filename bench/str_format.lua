-- String formatting: string.format, as a Lua programmer calls it (a local
-- alias of the library function).
local format = string.format
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 6000000

local total = 0
for _ = 1, scale do
  for i = 1, n do
    local s = format("%5d|%-8s|%.3f", i, "x" .. i % 100, i / 7)
    total = total + #s
  end
end
print(total)
