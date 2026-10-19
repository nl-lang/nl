# Bitwise operators: a xorshift32 generator and a population count by
# shifts and masks (`^`, `<<`, `>>>`, `&`, `|`), on 32-bit values.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 8000000

let acc = 0
for _rep in 1..scale
  let x = 2463534242
  for _i in 1..n
    x = x ^ (x << 13)
    x = x ^ (x >>> 17)
    x = x ^ (x << 5)
    let v = x & 0xffff
    v = v - ((v >>> 1) & 0x5555)
    v = (v & 0x3333) + ((v >>> 2) & 0x3333)
    v = (v + (v >>> 4)) & 0x0f0f
    acc = (acc + ((v + (v >>> 8)) & 0x1f)) | 0
  end
  acc = acc ^ x
end
print acc
