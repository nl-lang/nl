# A struct from another file is built field by field in declaration order, the parent's first: the default of an omitted or nil field, then the check
import { Monster, noted } from 'import/shapes'

let none: any = nil
let text: any = "high"

fn build(kind)
  case kind
  when 0
    Monster { id: 1 }
  when 1
    Monster { loot: "gem", id: 1, name: "bat" }
  when 2
    Monster { loot: "ore", level: none, id: 1, name: none }
  when 3
    Monster { id: 1, level: text }
  else
    Monster { boss: text, id: none }
  end
end

# What one construction gives, and the defaults it evaluated.
fn outcome(kind)
  let result
  try
    result = build(kind).describe
  catch e: TypeError
    result = e.message
  end
  "#{result} [#{noted()}]"
end

let counts = {}
for i in 0..499
  let o = outcome(i % 5)
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
