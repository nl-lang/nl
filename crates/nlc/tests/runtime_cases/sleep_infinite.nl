# Under `nlc run`, a sleep of infinity never
# wakes; NaN and a negative time wake at once. (Without the host's
# rt.resolve below, the run would end with the "still pending" report.)
let forever = Task.sleep(1 / 0)
forever.on_done do |v| print "infinite sleep: #{v}" end
Task.sleep(0 / 0).on_done do |v| print "NaN woke" end
Task.sleep(-1).on_done do |v| print "negative woke" end
Task.sleep(0.02).on_done do |v|
  print "short sleep woke; the infinite one is #{:NL_RT.taskstate(forever)}"
  :NL_RT.resolve(forever, "ended by the host")
end
