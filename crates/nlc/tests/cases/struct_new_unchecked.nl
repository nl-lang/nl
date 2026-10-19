# backends: lua,bc
# A literal of an imported struct is built by the runtime (rt.new, and
# the constructor it makes once the literal has run: runtime/
# nl_20_struct.lua), which checks a field exactly when the struct's file
# was compiled with checks: an unchecked build stores what a
# checked build refuses, and fills the defaults alike. Both builds print
# the same here, since each line is compared with what the build does for
# an annotated variable.
import { Piece, Crowned } from 'lib/pieces'
import from 'lib/pieces' as Pieces

let text: any = "high"
let none: any = nil

# Whether this build checks.
fn checks
  try
    let _n: Number = text
    false
  catch e: TypeError
    true
  end
end
let checked = checks()

fn mistyped
  Piece { rank: text }.rank
end
fn missing
  Pieces.Piece { name: "rook" }.rank
end
fn under
  Crowned { rank: 1, under: text, name: none }.under
end

# How many of `n` runs of `f` did what the build asks: a TypeError when it
# checks, the value `stored` when it does not.
fn agree(n, f, stored)
  let count = 0
  for _i in 1..n
    let raised = false
    let value
    try
      value = f()
    catch e: TypeError
      raised = true
    end
    count += 1 if raised == checked and (checked or value == stored)
  end
  count
end

print agree(200, mistyped!, "high")
print agree(200, missing!, nil)
print agree(200, under!, "high")

let sum = 0
for i in 1..200
  let c = Crowned { rank: i, name: none }
  let p = Pieces.Piece { under: c, rank: i }
  sum += p.rank + p.under.rank if c.crown and c.name == "pawn" and p.name == "pawn"
end
print sum
