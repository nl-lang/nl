# Time arithmetic: Time ± Duration, Time - Time, since/until, ordering, floor_to
let t = Time.at(1_792_504_980)
let later = t + 90.minutes
print later.iso(utc: true), (t - 1.days).iso(utc: true)
print typeof(later - t), later - t, t - later
print later.since(t), t.until(later), t.since(later)
print later > t, later < t, t <= t, t >= later, t == Time.at(1_792_504_980), t != later
print t.floor_to(1.hours).iso(utc: true), t.floor_to(15.minutes).iso(utc: true)
print t.floor_to(1.days).iso(utc: true), t.beginning_of_day(utc: true).iso(utc: true)
print((t + 0.5.seconds).floor_to(1.seconds) == t)
let times = [t + 3.hours, t, t + 1.hours]
:table.sort(times, |a, b| -> a < b)
for x in times
  print x.clock(utc: true)
end
case t + 0.seconds
when t
  print "same instant"
end
let one: any = 1
for f in [|| -> t + one, || -> t + t, || -> 1.days + t, || -> t - one, || -> t < one]
  try
    f()
  catch err: TypeError
    print err.message
  end
end
try
  t.floor_to(0.seconds)
catch err: TypeError
  print err.message
end
