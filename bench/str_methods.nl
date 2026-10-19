# String library methods on ASCII words: NL's `String` methods through a
# string receiver (`w.upcase`, `w.reverse`, `w.index`, `w.byteslice`,
# `w.start_with`, `w.length`, `w.strip`). NL's library counts characters
# (UTF-8), so it checks the encoding where Lua's counts bytes.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 3000000

let words = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel"]
let padded = ["  alpha ", " bravo", "charlie  ", "\tdelta\n"]

let check = 0
for _rep in 1..scale
  for i in 1..n
    let w = words[i % 8 + 1]
    let u = w.upcase
    let r = w.reverse
    check += u.len + r.len
    check += w.index("a") or 0
    check += w.byteslice(2, 4).len
    check += 1 if w.start_with("ch")
    check += w.length
    check += padded[i % 4 + 1].strip.len
  end
end
print check
