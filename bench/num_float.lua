-- Float arithmetic: the Mandelbrot set on a grid, counting the iterations
-- of every point (+ - * and comparisons on doubles, nested while loops).
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local size = 1000
local max_iter = 100

local total = 0
for _ = 1, scale do
  for py = 0, size - 1 do
    local ci = py * 2.0 / size - 1.0
    for px = 0, size - 1 do
      local cr = px * 2.5 / size - 2.0
      local zr, zi = 0.0, 0.0
      local n = 0
      while n < max_iter and zr * zr + zi * zi <= 4.0 do
        local t = zr * zr - zi * zi + cr
        zi = 2.0 * zr * zi + ci
        zr = t
        n = n + 1
      end
      total = total + n
    end
  end
end
print(total)
