# Time and Duration take finite numbers only, in a range where the calendar holds
let inf: any = 1 / 0
let nan: any = 0 / 0
let t = Time.at(0)
let cases = [
  || -> Time.at(inf),
  || -> Time.at(-inf),
  || -> Time.at(nan),
  || -> Duration.new(nan),
  || -> inf.minutes,
  || -> Time.new(2026, 1, 1, 0, 0, inf, utc: true),
  || -> 5.minutes * inf,
  || -> nan * 5.minutes,
  || -> 5.minutes / 0,
  || -> 5.minutes / 0.seconds,
  || -> 0.seconds / 0.seconds,
  || -> -t,
]
for f in cases
  try
    f()
    print "not reached"
  catch err: TypeError
    print err.message
  end
end
# Unspecified: an epoch below 1e15 seconds either way, a Duration below 2e15
print Time.at(1e15 - 1).iso(utc: true), Time.at(-1e15 + 1).date(utc: true)
let far: any = 1e300
let out_of_range = [
  || -> Time.at(1e15),
  || -> Time.at(far),
  || -> t + Duration.new(1e15),
  || -> Time.new(100_000_000_000, utc: true),
  || -> Duration.new(far),
  || -> 1e306.days,
]
for f in out_of_range
  try
    f()
    print "not reached"
  catch err: ConversionError
    print typeof(err)
  end
end
print 5.minutes / 2, 5.minutes / 10.seconds, (t + 1.days).iso(utc: true)
