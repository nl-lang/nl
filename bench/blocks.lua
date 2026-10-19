-- Blocks in hot loops: an `each`-style helper called with a closure that
-- captures the loop variable, a closure capturing an accumulator declared
-- outside the loop, and a closure inside a closure. Each closure is written
-- in the loop, as a Lua programmer writes a callback.
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 1000000

local function each(list, f)
  for i = 1, #list do f(list[i]) end
end

local function each_row(rows, f)
  for i = 1, #rows do f(rows[i]) end
end

local xs = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10}
local rows = {{1, 2, 3}, {4, 5, 6}, {7, 8, 9}}
local check = 0
local total = 0
for _ = 1, scale do
  for i = 1, n do
    each(xs, function(v) check = check + v * i end)
    total = 0
    each_row(rows, function(row)
      each(row, function(v) total = total + v + i end)
    end)
    check = (check + total) % 1000003
  end
end
print(check)
