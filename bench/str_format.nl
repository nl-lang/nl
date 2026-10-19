# String formatting: String.format through a string receiver
# ("...".format(args)) against Lua's string.format.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 6000000

let total = 0
for _rep in 1..scale
  for i in 1..n
    let s = "%5d|%-8s|%.3f".format(i, "x#{i % 100}", i / 7)
    total += s.len
  end
end
print total
