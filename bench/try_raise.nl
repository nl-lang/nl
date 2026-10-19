# `try`/`catch` where every iteration raises and catches: an exception
# struct raised with a field, caught by type, the field read.
const scale = (OS.getenv("NLB_SCALE") or "1") as Number
const n = 200000

struct BadInput : Exception
  code: Number
end

fn check_input(i)
  raise BadInput { message: "bad input", code: i % 17 }
end

let check = 0
for _rep in 1..scale
  for i in 1..n
    try
      check_input(i)
    catch e: BadInput
      check += e.code
    end
  end
end
print check
