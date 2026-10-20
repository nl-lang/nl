# Duration: the Number helpers and constructors, totals, parts, to_s, iso
let d = 90.minutes
print typeof(d), d is Duration
print d.total_seconds, d.total_minutes, d.total_hours, d.total_ms
print 36.hours.total_days, 250.ms.total_seconds, 1.5.seconds.total_ms
print 1.weeks.total_days, 2.days.total_hours, Duration.new(45).total_seconds
print Duration.seconds(5) == 5.seconds, Duration.minutes(2) == 120.seconds
print Duration.hours(1) == 60.minutes, Duration.days(1) == 24.hours
print Duration.weeks(1) == 7.days, Duration.ms(1500) == 1.5.seconds
let {days, hours, minutes, seconds, ms} = (2.days + 3.hours + 4.minutes + 5.5.seconds).parts
print days, hours, minutes, seconds, ms
let {minutes: m, seconds: s} = (-90).seconds.parts
print m, s
print 90.minutes, 2.days + 3.hours, 45.seconds, 0.seconds
print 250.ms, 1.5.seconds, (-45).seconds, 1.weeks
print "took #{1.minutes + 5.seconds}", 3.hours as String
print 90.minutes.iso, (2.days + 3.hours).iso, 0.25.seconds.iso
print 0.seconds.iso, 1.weeks.iso, (-1).hours.iso, 61.5.seconds.iso
