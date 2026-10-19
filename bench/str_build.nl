# String building and conversion: interpolation of numbers into strings,
# `<>` concatenation, `tostring`, and joining the pieces of a list with
# Table.concat. The checksum is the total length of what was built.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 200000

let total = 0
for _rep in 1..8 * scale
  let parts = []
  for i in 1..n
    let s = "item #{i}: #{i * 0.25} (#{i % 7})"
    let t = tostring(i) <> "/" <> tostring(n - i)
    parts[i] = s <> t
    total += s.len + t.len
  end
  total += Table.concat(parts, ",").len
end
print total
