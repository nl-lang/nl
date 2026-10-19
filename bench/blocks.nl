# Blocks in hot loops: an `each`-style helper called with a block that
# captures the loop variable, a block capturing an accumulator declared
# outside the loop, and a block inside a block. Each block is written in
# the loop, so the Lua twin makes a closure per iteration.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 1000000

fn each(list: [Number], f: Function)
  for v in list
    f(v)
  end
end

fn each_row(rows: [any], f: Function)
  for row in rows
    f(row)
  end
end

let xs = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
let rows = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
let check = 0
let total = 0
for _rep in 1..scale
  for i in 1..n
    each xs do |v|
      check += v * i
    end
    total = 0
    each_row rows do |row|
      each row do |v|
        total += v + i
      end
    end
    check = (check + total) % 1000003
  end
end
print check
