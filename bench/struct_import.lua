-- Object construction as struct_new.lua has it, for two classes, one with
-- a parent and a default filled in by the constructor function.
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 4000000

local Vec = {}
Vec.__index = Vec
local Thing = {}
Thing.__index = Thing
local Body = setmetatable({}, { __index = Thing })
Body.__index = Body

local function new_vec(x, y)
  return setmetatable({ x = x, y = y }, Vec)
end

local function new_body(id, pos, mass)
  return setmetatable({ id = id, pos = pos, mass = mass or 2 }, Body)
end

local function weigh(b)
  return b.mass * b.pos.x
end

local check = 0
for _ = 1, scale do
  local prev = new_vec(0, 0)
  for i = 1, n do
    local v = new_vec(prev.y + 1, i % 13)
    local b = new_body(i, v)
    check = check + weigh(b) + v.y
    prev = v
  end
  check = check % 1000003
end
print(check)
