# For v in x, x without a declared type: the loop takes its form from the value
fn items(x)
  let out = []
  for v in x
    Table.insert(out, tostring(v))
  end
  "[" <> Table.concat(out, " ") <> "]"
end

print items([10, 20, 30])
print items([1, nil, 3])
print items([true, false, 0])
print items([])
print items(2..4)
print items(2...4)
print items(3..1)
print items("aé→z")
print items("")

fn countdown(from)
  let n = from + 1
  fn()
    n -= 1
    return n if n > 0
  end
end
print items(countdown(3))

fn below(limit, i)
  return i + 1, "second" if i < limit
end
fn upto(limit)
  return below!, limit, 0
end
let firsts = []
for v in upto(3)
  Table.insert(firsts, v)
end
for i in ipairs(["p", "q"])
  Table.insert(firsts, i)
end
print Table.concat(firsts, " ")

struct Countdown
  from: Number
end
fn Countdown.iter
  countdown(@from)
end
print items(Countdown { from: 2 })

struct Steps
  limit: Number
end
fn Steps.iter
  return below!, @limit, 0, true, "more"
end
print items(Steps { limit: 2 })

let grow: any = [1, 2]
let visited = []
for v in grow
  Table.insert(visited, v)
  Table.insert(grow, v * 10) if grow.len < 5
end
print Table.concat(visited, " ")

let shrink: any = [1, 2, 3, 4]
let kept = []
for v in shrink
  Table.insert(kept, v)
  shrink[3] = nil if v == 1
end
print Table.concat(kept, " ")

let words: any = ["a", "skip", "b", "stop", "c"]
let seen = []
for w in words
  continue if w == "skip"
  break if w == "stop"
  Table.insert(seen, w)
end
print Table.concat(seen, " ")

fn first_over(xs, limit)
  for v in xs
    return v if v > limit
  end
  -1
end
print first_over([3, 8, 12], 5), first_over(1..4, 5)

let nums: any = [1, 2, 3]
let readers = []
for v in nums
  Table.insert(readers, fn() v end)
  v *= 10
end
print readers[1](), readers[2](), readers[3]()

let span: any = 1..3
let sum = 0
for v in span
  v += 100
  sum += v
end
print sum

let units: any = [{name: "orc", hp: 5}, {name: "elf", hp: 7}]
for {name, hp} in units
  print "#{name} #{hp}"
end

let broken: any = "ok\xff"
let count = 0
try
  for _ch in broken
    count += 1
  end
catch err: EncodingError
  print "EncodingError after #{count}"
end

let values: any = [5, true]
for bad in values
  try
    for _v in bad
      count += 1
    end
  catch err
    print typeof(bad), typeof(err), count
  end
end
