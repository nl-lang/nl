# Worker.register lets tables with a non-struct metatable cross
import { point_meta, read_point } from 'worker/registered'

async fn main
  let p = setmetatable({x: 3}, point_meta)
  let result = spawn read_point(p).await
  print result
end

main()
