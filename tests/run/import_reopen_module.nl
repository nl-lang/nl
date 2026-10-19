# `module Name` reopens an imported module
import { Tools } from 'import/lib'

module Tools
  fn thrice(x)
    x * 3
  end
end

print Tools.twice(2)
print Tools.thrice(2)
