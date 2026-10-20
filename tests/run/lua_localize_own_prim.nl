# the Lua paths of a primitive's method this file defines are read where they stand, whatever the localize mode
#!localize all

fn use
  "#{:math.twice(2)} #{:string.shout("a")} #{:String.shout("b")} #{:Table.second([1, 2])}"
end

fn Number.twice
  self * 2
end

fn String.shout
  self.upcase
end

fn Table.second
  self[2]
end

print use
print :math.floor(2.5), :string.rep("x", 2)
