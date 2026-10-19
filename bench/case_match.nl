# `case` against if-chains: value items (`when 1, 2`), ranges, `is T`,
# table patterns that bind fields, and a guard, over a list of messages.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 6000000

fn classify(code)
  case code
  when 0
    1
  when 1, 2
    2
  when 3..9
    3
  else
    4
  end
end

fn handle(msg)
  case msg
  when {kind: "move", dx, dy}
    dx + dy
  when {kind: "hit", amount} if amount > 50
    amount * 2
  when {kind: "hit", amount}
    amount
  when is String
    msg.len
  else
    0
  end
end

let msgs = [
  {kind: "move", dx: 1, dy: 2},
  {kind: "hit", amount: 70},
  {kind: "hit", amount: 10},
  "hello",
  {kind: "other"},
  {kind: "move", dx: -3, dy: 5},
]

let check = 0
for _rep in 1..scale
  for i in 1..n
    check += classify(i % 12)
    check += handle(msgs[i % 6 + 1])
  end
end
print check
