-- pcall in a hot loop where nothing raises: a protected call of a small
-- function that could fail, its value used.
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 10000000

local function parse_digit(c)
  if c < 0 or c > 9 then error("not a digit") end
  return c * 3 + 1
end

local check = 0
local failures = 0
for _ = 1, scale do
  for i = 1, n do
    local ok, v = pcall(parse_digit, i % 10)
    if not ok then
      failures = failures + 1
      v = 0
    end
    check = check + v
  end
end
print(check, failures)
