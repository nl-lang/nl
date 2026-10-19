# Worker.run, Worker.map, Worker.count and Worker.parent
import { square, add } from 'worker/jobs'

async fn main
  let handlers = {sq: square!, ad: add!}
  let a = Worker.run(handlers["sq"], 6).await
  print a
  let b = Worker.run(handlers.ad, 1, 2).await
  print b
  let squares = Worker.map([1, 2, 3], square!).await
  print Table.concat(squares, ",")
  let sums = Worker.map([1, 2, 3], add!, 100).await
  print Table.concat(sums, ",")
  print typeof(Worker.count)
  print Worker.parent == nil
end

main()
