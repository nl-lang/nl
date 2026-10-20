# Local time and Time.now: relations that hold in every time zone
let t = Time.at(1_784_116_800)
let off = t.utc_offset
print typeof(off), off.total_seconds % 60 == 0, off.abs < 1.days
let wall = t + off
print t.hour == wall.hour(utc: true), t.min == wall.min(utc: true), t.day == wall.day(utc: true)
print t.date == wall.date(utc: true), t.clock == wall.clock(utc: true)
print t.strftime("%F %T") == wall.strftime("%F %T", utc: true)
print t.wday == wall.wday(utc: true), t.yday == wall.yday(utc: true)
print Time.parse(t.iso) == t, Time.parse(t.to_s) == t, "#{t}" == t.iso
print Time.new(t.year, t.month, t.day, t.hour, t.min, t.sec) == t
print Time.parse(t.date) == t.beginning_of_day, t.beginning_of_day <= t
print t.beginning_of_day.hour, t.beginning_of_day.min
let now = Time.now
print now is Time, now > Time.at(1_700_000_000), now.epoch is Number
print 5.minutes.ago < Time.now, 5.minutes.from_now > Time.now
print Time.now - now >= 0.seconds, Time.now.year >= 2026
