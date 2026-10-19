# The script of the example host.
let ticks = 0

on Game.Tick do |dt: Number|
  ticks += 1
  print "tick #{ticks}" if ticks % 10 == 0
end

async fn load(url: String)
  let request = :http_get(url)
  let body = request.await
  print "fetched #{body}"
end

load("https://example.com/a")
print "main.nl loaded"
