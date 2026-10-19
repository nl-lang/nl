# Task.all keeps list order; Task.race takes the first task to finish
let resolvers = []

fn deferred
  Task.new do |resolve, reject|
    Table.insert(resolvers, resolve)
  end
end

let a = deferred()
let b = deferred()
Task.all([a, b]).on_done do |values|
  print "all: #{values[1]} #{values[2]}"
end

let c = deferred()
let d = deferred()
Task.race([c, d]).on_done do |v|
  print "race: #{v}"
end

resolvers[2]("b")
print "after b"
resolvers[1]("a")
resolvers[4]("d")
resolvers[3]("c")
print "end"
