-- Field access: particles as plain tables, updated through p.x, p.vx, ...
local floor = math.floor
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 1000
local steps = 20000

local check = 0
for _ = 1, scale do
  local ps = {}
  for i = 1, n do
    ps[i] = { x = i, y = i * 0.5, vx = (i % 7) - 3, vy = (i % 5) - 2 }
  end
  for _ = 1, steps do
    for i = 1, n do
      local p = ps[i]
      p.x = p.x + p.vx * 0.01
      p.y = p.y + p.vy * 0.01
      if p.x < 0 or p.x > 1000 then
        p.vx = -p.vx
      end
      if p.y < 0 or p.y > 1000 then
        p.vy = -p.vy
      end
    end
  end
  for i = 1, n do
    check = check + ps[i].x + ps[i].y
  end
end
print(floor(check))
