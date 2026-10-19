# What crosses: copies, shared references and cycles, structs, enums, ranges, named functions, exceptions
import { mutate, same_table, has_cycle, describe, range_sum, apply_fn, square, fail_with, JobError, Rect } from 'worker/jobs'

enum Color
  Red
  Blue
end

async fn main
  let data = [1, 2]
  let size = spawn mutate(data).await
  print size
  print data.changed == nil
  let shared = {}
  let same = spawn same_table({a: shared, b: shared}).await
  print same
  let cyc = {}
  cyc.again = cyc
  let cycle_ok = spawn has_cycle(cyc).await
  print cycle_ok
  let rect_kind = spawn describe(Rect { w: 1, h: 1 }).await
  print rect_kind
  let enum_kind = spawn describe(Color.Red).await
  print enum_kind
  let r = spawn range_sum(1..4).await
  print r
  let fsq = spawn apply_fn(square!, 5).await
  print fsq
  try
    let x = spawn fail_with("bad input").await
  catch err: JobError
    print "#{typeof(err)} #{err.reason}"
    print err.trace is String
  end
  let bad: any = "x"
  try
    let y = spawn square(bad).await
  catch err: TypeError
    print "TypeError from the worker"
  end
end

main()
