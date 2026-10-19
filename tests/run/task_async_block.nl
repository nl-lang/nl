# async do blocks and async event handlers
let resolvers = []

fn deferred
  Task.new do |resolve, reject|
    Table.insert(resolvers, resolve)
  end
end

fn start(f: Function)
  f()
end

let t = start async do
  let v = deferred().await
  print "block got #{v}"
  v + 1
end
t.on_done do |r|
  print "block task done #{r}"
end

on Joined as 'greeter' async do |name|
  let v = deferred().await
  print "welcome #{name}, #{v}"
end

Event.run "Joined", "kim"
print "handler is waiting"
resolvers[1](41)
resolvers[2]("late")
