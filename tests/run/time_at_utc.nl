# Time.at and the calendar in UTC: accessors, epoch, iso, date, clock
let t = Time.at(1_792_504_980)
print typeof(t), t is Time, t.epoch
print t.year(utc: true), t.month(utc: true), t.day(utc: true)
print t.hour(utc: true), t.min(utc: true), t.sec(utc: true)
print t.wday(utc: true), t.yday(utc: true)
print t.iso(utc: true), t.date(utc: true), t.clock(utc: true)
print Time.at(0).iso(utc: true), Time.at(0).wday(utc: true)
print Time.at(-1).iso(utc: true), Time.at(1.25).iso(utc: true)
print Time.at(951_782_400).date(utc: true), Time.at(951_782_400).yday(utc: true)
print Time.at(4_107_542_400).iso(utc: true)
print Time.at(-2_208_988_800).iso(utc: true), Time.at(-2_208_988_800).wday(utc: true)
let days = []
for i in 0..6
  days[i + 1] = (t + i.days).wday(utc: true)
end
print Table.concat(days, " ")
print t.year(utc: false) is Number
