-- Mixed workload: a stack-machine interpreter. The program is a list of
-- instruction tables; the dispatch is an if-chain on the opcode name, the
-- stack and the variables are tables indexed by number.
local scale = tonumber(os.getenv("NLB_SCALE") or "1")

-- Sums i * i % 7 for i = 1..limit, as instructions.
local function program(limit)
  return {
    { op = "push", arg = 0 }, { op = "store", arg = 1 },      -- sum = 0
    { op = "push", arg = 1 }, { op = "store", arg = 2 },      -- i = 1
    { op = "load", arg = 2 }, { op = "push", arg = limit },   -- 5: loop head
    { op = "gt" }, { op = "jumpif", arg = 22 },
    { op = "load", arg = 1 }, { op = "load", arg = 2 }, { op = "load", arg = 2 },
    { op = "mul" }, { op = "push", arg = 7 }, { op = "mod" }, { op = "add" },
    { op = "store", arg = 1 },
    { op = "load", arg = 2 }, { op = "push", arg = 1 }, { op = "add" }, { op = "store", arg = 2 },
    { op = "jump", arg = 5 },
    { op = "load", arg = 1 }, { op = "halt" },               -- 22
  }
end

local function run(code)
  local stack, vars = {}, {}
  local sp, pc = 0, 1
  while true do
    local ins = code[pc]
    pc = pc + 1
    local op = ins.op
    if op == "push" then
      sp = sp + 1
      stack[sp] = ins.arg
    elseif op == "load" then
      sp = sp + 1
      stack[sp] = vars[ins.arg]
    elseif op == "store" then
      vars[ins.arg] = stack[sp]
      sp = sp - 1
    elseif op == "add" then
      stack[sp - 1] = stack[sp - 1] + stack[sp]
      sp = sp - 1
    elseif op == "mul" then
      stack[sp - 1] = stack[sp - 1] * stack[sp]
      sp = sp - 1
    elseif op == "mod" then
      stack[sp - 1] = stack[sp - 1] % stack[sp]
      sp = sp - 1
    elseif op == "gt" then
      stack[sp - 1] = stack[sp - 1] > stack[sp]
      sp = sp - 1
    elseif op == "jumpif" then
      if stack[sp] then pc = ins.arg end
      sp = sp - 1
    elseif op == "jump" then
      pc = ins.arg
    elseif op == "halt" then
      return stack[sp]
    end
  end
end

local check = 0
for _ = 1, scale do
  check = check + run(program(2000000))
end
print(check)
