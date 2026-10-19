# spawn do in a module function, a loop, a method, a generic function, and nested
import { square } from 'worker/jobs'

module Geo
  struct Box
    w: Number
  end

  fn grow(n: Number): Number
    n + 1
  end

  fn boxed(n: Number)
    let t = spawn do
      Box { w: grow(n) }
    end
    t.await
  end
end

struct Counter
  step: Number
end

fn Counter.ahead(n)
  let s = @step
  let t = spawn do
    n + s
  end
  t.await
end

fn twice<T>(v: T): [T]
  let t = spawn do
    [v, v]
  end
  t.await
end

async fn main
  let b = Geo.boxed(4)
  print "#{typeof(b)} #{b.w}"
  let tasks = []
  for i in 1..3
    tasks[i] = spawn do
      i * 10
    end
  end
  print Table.concat(Task.all(tasks).await, ",")
  let c = Counter { step: 5 }
  print c.ahead(2)
  print Table.concat(twice("ab"), ",")
  let outer = spawn do
    let inner = spawn do
      square(3)
    end
    inner.await + 1
  end
  print outer.await
end

main() unless Worker.parent
