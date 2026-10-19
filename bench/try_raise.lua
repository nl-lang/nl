-- pcall where every iteration raises and catches: an error table with a
-- field, recognized by its metatable, the field read.
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 200000

local BadInput = {}
BadInput.__index = BadInput

local function check_input(i)
  error(setmetatable({ message = "bad input", code = i % 17 }, BadInput))
end

local check = 0
for _ = 1, scale do
  for i = 1, n do
    local ok, e = pcall(check_input, i)
    if not ok then
      if getmetatable(e) == BadInput then
        check = check + e.code
      else
        error(e, 0)
      end
    end
  end
end
print(check)
