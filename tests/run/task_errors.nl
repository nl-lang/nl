# Rejection, try/catch across .await, on_error, and .await outside a task
let resolvers = []
let rejecters = []

fn deferred
  Task.new do |resolve, reject|
    Table.insert(resolvers, resolve)
    Table.insert(rejecters, reject)
  end
end

async fn guarded
  try
    deferred().await
    print "not reached"
  catch err
    print "caught across await: #{err.message}"
  end
  "guarded finished"
end

let g = guarded()
g.on_done do |v|
  print v
end
rejecters[1](Exception { message: "rejected" })

let failing = deferred()
failing.on_error do |err|
  print "on_error: #{err.message}"
end
rejecters[2](Exception { message: "second" })

let later = deferred()
try
  later.await
catch
  print "await outside a task raised"
end
resolvers[3](1)
