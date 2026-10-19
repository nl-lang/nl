# Mixed workload: a stack-machine interpreter. The program is a list of
# instruction tables; the dispatch is a `case` on the opcode name, the
# stack and the variables are lists indexed by number.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number

# Sums i * i % 7 for i = 1..limit, as instructions.
fn program(limit)
  [
    {op: "push", arg: 0}, {op: "store", arg: 1},      # sum = 0
    {op: "push", arg: 1}, {op: "store", arg: 2},      # i = 1
    {op: "load", arg: 2}, {op: "push", arg: limit},   # 5: loop head
    {op: "gt"}, {op: "jumpif", arg: 22},
    {op: "load", arg: 1}, {op: "load", arg: 2}, {op: "load", arg: 2},
    {op: "mul"}, {op: "push", arg: 7}, {op: "mod"}, {op: "add"},
    {op: "store", arg: 1},
    {op: "load", arg: 2}, {op: "push", arg: 1}, {op: "add"}, {op: "store", arg: 2},
    {op: "jump", arg: 5},
    {op: "load", arg: 1}, {op: "halt"},               # 22
  ]
end

fn run(code)
  let stack = []
  let vars = []
  let sp = 0
  let pc = 1
  loop
    let ins = code[pc]
    pc += 1
    case ins.op
    when "push"
      sp += 1
      stack[sp] = ins.arg
    when "load"
      sp += 1
      stack[sp] = vars[ins.arg]
    when "store"
      vars[ins.arg] = stack[sp]
      sp -= 1
    when "add"
      stack[sp - 1] = stack[sp - 1] + stack[sp]
      sp -= 1
    when "mul"
      stack[sp - 1] = stack[sp - 1] * stack[sp]
      sp -= 1
    when "mod"
      stack[sp - 1] = stack[sp - 1] % stack[sp]
      sp -= 1
    when "gt"
      stack[sp - 1] = stack[sp - 1] > stack[sp]
      sp -= 1
    when "jumpif"
      pc = ins.arg if stack[sp]
      sp -= 1
    when "jump"
      pc = ins.arg
    when "halt"
      return stack[sp]
    end
  end
end

let check = 0
for _rep in 1..scale
  check += run(program(2000000))
end
print check
