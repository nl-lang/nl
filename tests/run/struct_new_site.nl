# One literal whose path names another struct from run to run: a field that one struct has and the other lacks, a value that is not a struct
import { Vec, Tagged, Mode } from 'import/shapes'

module Kinds
end

fn plain(i)
  Kinds.Pick { x: i, y: 1 }
end

fn named(i)
  Kinds.Pick { name: "n", y: 1, x: i }
end

# What `f` gives when `Kinds.Pick` is `pick`.
fn outcome(pick, f)
  Kinds.Pick = pick
  try
    let o = f(2)
    "#{typeof(o)} #{o.sum}"
  catch e
    "#{typeof(e)}: #{e.message}"
  end
end

let picks = [Vec, Tagged, 5, Vec { x: 0, y: 0 }, Kinds, Mode]
let counts = {}
fn count(o)
  counts[o] = (counts[o] or 0) + 1
end
for i in 0..599
  let pick = picks[i % 6 + 1]
  count("plain: " <> outcome(pick, plain!))
  count("named: " <> outcome(pick, named!))
  count("plain: " <> outcome(nil, plain!)) if i % 6 == 0
end
let outcomes = []
for o, _ in counts
  Table.insert(outcomes, o)
end
Table.sort(outcomes)
for o in outcomes
  print "#{counts[o]} x #{o}"
end
