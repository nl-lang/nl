# Times and Durations cross to a worker and back as values, and their types by name
import { identity, describe } from 'worker/jobs'

async fn main
  let t = Time.at(1_792_504_980)
  let back = spawn identity(t).await
  print typeof(back), back == t, back.iso(utc: true)
  let d = spawn identity(90.minutes).await
  print typeof(d), d, d == 90.minutes
  print spawn describe(t).await, spawn describe(2.days).await
  let both = spawn identity({at: t, wait: 5.seconds, types: [Time, Duration]}).await
  let due = both.at + both.wait
  print due.clock(utc: true)
  print both.types[1] == Time, both.types[2] == Duration
end

main() unless Worker.parent
