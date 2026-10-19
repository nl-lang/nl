# Lua paths to library functions in a hot loop: `:math.floor` and
# `:math.sin` written at each use (read once, into hidden
# locals, under the default `--localize=std`), against the Lua idiom of a
# local per library function.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 10000000

fn run(count)
  let total = 0
  for i in 1..count
    total += :math.floor(:math.sin(i) * 1000) + :math.floor(i / 3)
  end
  total
end

let total = 0
for _rep in 1..scale
  total += run(n)
end
print total
