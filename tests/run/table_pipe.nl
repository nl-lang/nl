# The pipe and the method form of Table's functions, mixed: both pass the table first
let scores: [Number] = [72, 95, 58, 88, 61]

let passed = scores |> Table.filter(s -> s >= 60) |> Table.sorted
print passed.join(" ")

# a method on the result of a pipe, and a pipe on the result of a method
let tens = scores |> Table.map(s -> s // 10)
print tens.uniq.join(",")
print scores.map(s -> s + 5) |> Table.max

# a plain function in a chain of Table functions
fn grade(s)
  if s >= 90
    "A"
  elseif s >= 70
    "B"
  else
    "C"
  end
end
let grades = scores.map(grade!) |> Table.tally
grades.each_sorted(|g, n| -> print(g, n))

# an untyped value goes through the pipe, which needs no type
fn top(list, n)
  list |> Table.sorted(|a, b| -> a > b) |> Table.take(n)
end
print top(scores, 2) |> Table.join(",")

let words = "the quick brown fox jumps over the lazy dog".split(" ")
let by_len = words.uniq.group_by(w -> w.len)
by_len.each_sorted do |len, ws|
  print len, Table.join(ws, " ")
end
print words |> Table.count("the"), words.find_index(w -> w.start_with("f"))
