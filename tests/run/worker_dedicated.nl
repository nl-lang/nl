# A dedicated worker keeps state; calls return tasks; emit runs events in either direction
const svc = Worker.new 'worker/counter_service'

on Pong do |v|
  print "pong #{v}"
end

async fn main
  let first = svc.add(5).await
  print first
  let second = svc.add(3).await
  print second
  svc.emit "Ping", 21
  let total = svc.get_total().await
  print total
  svc.stop()
end

main()
