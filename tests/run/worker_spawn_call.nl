# spawn, call form (plain, defaults, command form, no arguments, method, async callee)
import { square, add, answer, Rect, double_square } from 'worker/jobs'

async fn main
  let t = spawn square(7)
  print t.await
  let b = spawn add(5).await
  print b
  let c = spawn add(1, 2).await
  print c
  let cmd = spawn add 3, 4
  print cmd.await
  let z = spawn answer().await
  print z
  let r = Rect { w: 2, h: 3 }
  let area = spawn r.area().await
  print area
  let ds = spawn double_square(3).await
  print ds
end

main()
