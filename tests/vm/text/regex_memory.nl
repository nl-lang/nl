# The memory of a compiled pattern is charged to the collector and taken off when it is collected
let before = :collectgarbage("count")
let pats = []
let i = 0
while i < 300
  i += 1
  Table.insert(pats, Regex.new("p#{i}[a-z]+\\d*"))
end
let during = :collectgarbage("count")
print during - before > 100
pats = nil
:collectgarbage("collect")
:collectgarbage("collect")
let after = :collectgarbage("count")
print after < during
print during - after > (during - before) / 2
