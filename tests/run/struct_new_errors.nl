# The TypeError of a literal of a struct from another file, for a missing or mistyped field, with the line of the literal, each time it runs
import { Vec, Game, Mode, Entity, Monster } from 'import/shapes'
import from 'import/shapes' as Shapes

let text: any = "one"
let v: any = Vec { x: 1, y: 2 }

fn attempt(kind)
  case kind
  when 0
    Vec { x: 1 }
  when 1
    Shapes.Vec { y: 2, x: text }
  when 2
    Game { mode: text, seed: 1, first: Entity { id: 1 } }
  when 3
    Game { mode: Mode.Hard, seed: v, first: Entity { id: 1 } }
  when 4
    Game { mode: Mode.Hard, seed: text, first: v }
  else
    typeof(Game { mode: Mode.Easy, seed: 5, first: Monster { id: 2 } }.first)
  end
end

# What one construction gives: a value, or the error and the first line of its trace.
fn outcome(kind)
  try
    attempt(kind)
  catch e: TypeError
    "#{e.message} (#{e.trace.split("\n")[2].strip})"
  end
end

let counts = {}
for i in 0..599
  let o = outcome(i % 6)
  counts[o] = (counts[o] or 0) + 1
end
let outcomes = []
for o, _ in counts
  Table.insert(outcomes, o)
end
Table.sort(outcomes)
for o in outcomes
  print "#{counts[o]} x #{o}"
end
