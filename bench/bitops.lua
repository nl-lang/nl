-- Bitwise operators: a xorshift32 generator and a population count by
-- shifts and masks (bit.bxor, bit.lshift, bit.rshift, bit.band, bit.bor),
-- on 32-bit values.
local bxor, lshift, rshift, band, bor = bit.bxor, bit.lshift, bit.rshift, bit.band, bit.bor
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 8000000

local acc = 0
for _ = 1, scale do
  local x = 2463534242
  for _ = 1, n do
    x = bxor(x, lshift(x, 13))
    x = bxor(x, rshift(x, 17))
    x = bxor(x, lshift(x, 5))
    local v = band(x, 0xffff)
    v = v - band(rshift(v, 1), 0x5555)
    v = band(v, 0x3333) + band(rshift(v, 2), 0x3333)
    v = band(v + rshift(v, 4), 0x0f0f)
    acc = bor(acc + band(v + rshift(v, 8), 0x1f), 0)
  end
  acc = bxor(acc, x)
end
print(acc)
