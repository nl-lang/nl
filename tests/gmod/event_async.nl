# An async `on` handler waits on a timer; the engine's Think hook pumps the runtime
on PlayerSay as 'chat' async do |who, text|
  Task.sleep(0.5).await
  print "#{who} said #{text}, half a second ago"
end

let result = :hook.Run("PlayerSay", "bob", "hi")
print(result == nil)
print "the handler is waiting"
