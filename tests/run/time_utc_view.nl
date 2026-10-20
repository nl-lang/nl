# t.utc: the same instant, whose calendar reads in UTC without `utc: true`; t.local reads it in local time
let t = Time.at(1_792_504_980.25)
let u = t.utc
print u.is_utc, t.is_utc, u.local.is_utc, u.utc.is_utc
print u.epoch == t.epoch, u == t, u.local == t, u <= t, u.since(t)
print u, "#{u}", u.iso, u.date, u.clock
print u.year, u.month, u.day, u.hour, u.min, u.sec, u.wday, u.yday
print u.strftime("%F %T.%L %Z %z"), u.utc_offset
# what is computed from a UTC view is one too
let later = u + 90.minutes
print later.is_utc, later, (u - 1.days).date, u.beginning_of_day, u.floor_to(1.hours)
print u.beginning_of_day.is_utc, u.floor_to(1.hours).is_utc, (later - u).to_s
# an explicit option still decides one call
print u.hour(utc: true), u.iso(utc: true) == u.iso, t.iso(utc: true) == u.iso
let wall = t + t.utc_offset
print u.hour(utc: false) == wall.hour(utc: true), u.local.clock == wall.clock(utc: true)
print u.local.to_s == t.to_s, u.local.utc_offset == t.utc_offset
# constructors give local views, but Time.new and Time.parse given utc: true give UTC ones
print Time.at(0).is_utc, Time.new(2026, 1, 1, utc: true).is_utc, Time.parse("2026-01-01T00:00:00Z").is_utc
let made = Time.new(2020, 1, 1, utc: true)
print made.hour, made.iso, Time.parse("2020-01-01T00:00:00+02:00", utc: true).iso
print Time.parse("2020-01-01", utc: true).is_utc, Time.new(2020, utc: false).is_utc, Time.parse("2020-01-01", utc: false).is_utc
print Time.parse("2026-01-01T00:00:00Z").utc, Time.new(2026, 10, 20, 14, 3, utc: true).utc == u.floor_to(1.minutes)
let view: any = Time.at(86_400).utc
print view.is_utc, view.date, view.local.is_utc
