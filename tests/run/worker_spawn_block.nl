# spawn do blocks: captured copies, return, file-level declarations, per-state variables
import { square } from 'worker/jobs'

const factor = 3
let file_level = "initial"

fn helper(x)
  x + 1
end

async fn main
  file_level = "changed in main"
  let n = 4
  let list = [1, 2, 3]
  let sum = spawn do
    let total = 0
    for v in list
      total += v * n
    end
    total
  end
  print sum.await
  let early = spawn do
    return "returned early" if n > 0
    "not reached"
  end
  print early.await
  let uses_decls = spawn do
    helper(factor)
  end
  print uses_decls.await
  let nested = spawn do
    let inner = spawn square(n)
    inner.await + 1
  end
  print nested.await
  let own_copy = spawn do
    file_level
  end
  print own_copy.await
  print file_level
  let has_parent = spawn do
    Worker.parent != nil
  end
  print has_parent.await
  print Worker.parent == nil
end

main() unless Worker.parent
