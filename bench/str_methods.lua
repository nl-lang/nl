-- String library functions on ASCII words: Lua's string table through
-- method calls on strings (s:upper(), s:reverse(), s:find(), s:sub(),
-- #s, a prefix test with sub, and a trim with match).
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 3000000

local words = { "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel" }
local padded = { "  alpha ", " bravo", "charlie  ", "\tdelta\n" }

local check = 0
for _ = 1, scale do
  for i = 1, n do
    local w = words[i % 8 + 1]
    local u = w:upper()
    local r = w:reverse()
    check = check + #u + #r
    check = check + (w:find("a", 1, true) or 0)
    check = check + #w:sub(2, 4)
    if w:sub(1, 2) == "ch" then check = check + 1 end
    check = check + #w
    check = check + #padded[i % 4 + 1]:match("^%s*(.-)%s*$")
  end
end
print(check)
