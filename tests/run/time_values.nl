# Time and Duration as types: is, typeof, case, checked parameters, immutability
fn describe(v)
  case v
  when is Time
    "a time at #{v.epoch}"
  when is Duration
    "a duration of #{v}"
  else
    typeof(v)
  end
end
print describe(Time.at(5)), describe(3.days), describe(5)
fn deadline(start: Time, budget: Duration): Time
  start + budget
end
print deadline(Time.at(0), 2.hours).clock(utc: true)
let text: any = "soon"
try
  deadline(Time.at(0), text)
catch err: TypeError
  print err.message
end
let plain: any = {1: 5}
print plain is Time, 5 is Duration, Time.at(1) is Duration
let t = Time.at(0)
let d = 1.minutes
for f in [fn() t.label = "x" end, fn() d.extra = 1 end]
  try
    f()
  catch err: TypeError
    print err.message
  end
end
print t.epoch, d.total_seconds
let any_t: any = Time.at(60)
print any_t.min(utc: true), any_t.iso(utc: true)
let r = Time
print r.at(120).min(utc: true), Duration.minutes(2) == 2.minutes
print "#{d} after #{t.iso(utc: true)}", d as String
let by_name = {}
by_name[typeof(t)] = 1
by_name[typeof(d)] = 2
print by_name.Time, by_name.Duration
