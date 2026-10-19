-- Method calls: a class as a metatable with __index, called with `:`.
local floor = math.floor
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 6000000

local Vec = {}
Vec.__index = Vec

local function new_vec(x, y)
  return setmetatable({ x = x, y = y }, Vec)
end

function Vec:norm2()
  return self.x * self.x + self.y * self.y
end

function Vec:add(o)
  self.x = self.x + o.x
  self.y = self.y + o.y
end

function Vec:dot(o)
  return self.x * o.x + self.y * o.y
end

local check = 0
local xy = 0
for _ = 1, scale do
  local acc = new_vec(0, 0)
  local v = new_vec(1, 2)
  local w = new_vec(0.5, -0.25)
  for _ = 1, n do
    acc:add(w)
    check = check + acc:dot(v) * 0.001 + v:norm2()
    check = check % 1000003
  end
  xy = xy + acc.x + acc.y
end
print(floor(check * 1000), xy)
