-- If-chains for the same dispatch: comparisons for the codes, type()
-- and field tests for the messages.
local scale = tonumber(os.getenv("NLB_SCALE") or "1")
local n = 6000000

local function classify(code)
  if code == 0 then
    return 1
  elseif code == 1 or code == 2 then
    return 2
  elseif code >= 3 and code <= 9 then
    return 3
  else
    return 4
  end
end

local function handle(msg)
  if type(msg) == "table" then
    local kind = msg.kind
    if kind == "move" and msg.dx ~= nil and msg.dy ~= nil then
      return msg.dx + msg.dy
    elseif kind == "hit" and msg.amount ~= nil then
      local amount = msg.amount
      if amount > 50 then return amount * 2 end
      return amount
    end
  elseif type(msg) == "string" then
    return #msg
  end
  return 0
end

local msgs = {
  { kind = "move", dx = 1, dy = 2 },
  { kind = "hit", amount = 70 },
  { kind = "hit", amount = 10 },
  "hello",
  { kind = "other" },
  { kind = "move", dx = -3, dy = 5 },
}

local check = 0
for _ = 1, scale do
  for i = 1, n do
    check = check + classify(i % 12)
    check = check + handle(msgs[i % 6 + 1])
  end
end
print(check)
