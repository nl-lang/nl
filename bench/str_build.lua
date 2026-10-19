-- String building and conversion: numbers concatenated into strings with
-- `..`, `tostring`, and joining the pieces of a list with table.concat.
-- The checksum is the total length of what was built.
local concat = table.concat
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 200000

local total = 0
for _ = 1, 8 * scale do
  local parts = {}
  for i = 1, n do
    local s = "item " .. i .. ": " .. i * 0.25 .. " (" .. i % 7 .. ")"
    local t = tostring(i) .. "/" .. tostring(n - i)
    parts[i] = s .. t
    total = total + #s + #t
  end
  total = total + #concat(parts, ",")
end
print(total)
