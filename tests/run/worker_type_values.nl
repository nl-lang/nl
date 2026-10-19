# Enums, modules and interfaces cross by name, as structs do
import { identity } from 'worker/jobs'
import { Shade, Tools, Named } from 'worker/types'

async fn main
  let e = spawn identity(Shade).await
  print e == Shade
  print e.Dark == Shade.Dark
  let m = spawn identity(Tools).await
  print m == Tools
  print m.double(21)
  let i = spawn identity({kind: Named}).await
  print i.kind == Named
end

main()
