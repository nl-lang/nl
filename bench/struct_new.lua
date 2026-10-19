-- Object construction: setmetatable on a table constructor, with a
-- default filled in by the constructor function.
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 6000000

local Vec = {}
Vec.__index = Vec

local function new_vec(x, y, w)
  return setmetatable({ x = x, y = y, w = w or 1 }, Vec)
end

local check = 0
for _ = 1, scale do
  local prev = new_vec(0, 0)
  for i = 1, n do
    local v = new_vec(prev.y + 1, i % 13)
    check = check + v.x + v.y * v.w
    prev = v
  end
  check = check % 1000003
end
print(check)
