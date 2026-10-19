-- Closure creation and calls: a closure made per iteration (capturing a
-- loop variable), called a few times, and an anonymous function passed to
-- a higher-order function.
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 8000000

local function make_adder(k)
  return function(x) return x + k end
end

local function apply_twice(x, f)
  return f(f(x))
end

local check = 0
for _ = 1, scale do
  for i = 1, n do
    local add = make_adder(i % 100)
    check = check + add(1) + add(2)
    check = check + apply_twice(i % 10, function(v) return v * 2 + 1 end)
    check = check % 1000003
  end
end
print(check)
