# Localized library paths: the paths a file reads once into
# hidden locals give the values they gave before, the paths it assigns are
# read where they stand, and globals stay variables. The output is the same
# with --localize=none, std and all (crates/nlc/tests/conformance.rs).
import { hit } from 'import/localized'
global Level = 1

# Reads of library fields, at the top level and in a command call.
print :string.format("%5.2f|%s", :math.pi, :string.rep("ab", 3))
print :math.max(3, 9, 4), :math.huge > 1e308, :string.byte("A")
let parts = []
:table.insert parts, "x"
:table.insert(parts, "y")
print :table.concat(parts, ",")

# A function the imported file added to `string` when it ran.
print :string.shout("hey")

# Uses inside nested functions, a loop, a block and a try.
fn circle(r)
  let area = fn(x)
    :math.pi * x * x
  end
  :math.floor(area(r) * 100) / 100
end

fn digits(n)
  let out = []
  for i in 1..n
    out[out.len + 1] = :string.char(48 + i % 10)
  end
  :table.concat(out)
end

fn each(list: Table, f: Function)
  for x in list
    f(x)
  end
end

print circle(2), digits(12)
let total = 0
each([1.5, 2.5, 3.5]) do |x|
  total += :math.ceil(x)
end
print total
let safe = try
  :math.sqrt(16) + :string.len(nil)
catch e
  -1
end
print safe

# The file assigns `:math.floor`: its uses read the global, before and
# after, and see the assignment.
let real_floor = :math.floor
print :math.floor(2.7)
:math.floor = fn(x) 42 end
print :math.floor(2.7), circle(1)
:math.floor = real_floor
print :math.floor(2.7)

# Globals stay variables: this file's own, and the imported file's.
fn bump
  Level += 1
end
bump()
print :Level, :_G.Level
print hit(), :LocalizedHits
print hit(), :LocalizedHits

# A longer path starts from the localized value, also when it writes into
# it (the imported file made `string.counts`).
:string.counts.local = 5
print :string.counts.local, :string.counts.imported
let co = :coroutine.wrap(fn()
  :coroutine.yield(7)
end)
print co()
