# async fn starts at once and returns a Task; Task.new, .await and on_done
let pending = []

fn deferred
  Task.new do |resolve, reject|
    Table.insert(pending, resolve)
  end
end

async fn work(x)
  print "start #{x}"
  let v = deferred().await
  print "resumed #{x} with #{v}"
  v * x
end

let t = work(2)
print "after call"
t.on_done do |r|
  print "done #{r}"
end
pending[1](10)
print "end of main"
