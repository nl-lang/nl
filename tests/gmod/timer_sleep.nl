# Task.sleep is timer.Simple: sleeps end in the order of their deadlines, in game time
async fn nap(name: String, seconds: Number)
  let before = :CurTime()
  Task.sleep(seconds).await
  let slept = :CurTime() - before
  print "#{name} woke after at least #{seconds}s: #{slept >= seconds}"
  name
end

let all = Task.all([nap("long", 2), nap("short", 0.1), nap("zero", 0)])
all.on_done do |names|
  print "all done: #{Table.concat(names, ", ")}"
end

# A Lua timer and an NL task in the same frames.
:timer.Simple(1, || -> print("Lua timer after 1s"))

async fn chain
  for i in 1..3
    Task.sleep(0.25).await
    print "tick #{i}"
  end
end
chain()
