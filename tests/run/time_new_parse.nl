# Time.new and Time.parse in UTC; carry-over of components, parse errors
let t = Time.new(2026, 10, 20, 14, 3, utc: true)
print t.iso(utc: true), t.epoch
print Time.new(2026, utc: true).iso(utc: true), Time.new(2026, 2, 29, utc: true).date(utc: true)
print Time.new(2026, 13, 1, utc: true).date(utc: true), Time.new(2026, 3, 0, utc: true).date(utc: true)
print Time.new(2026, 10, 20, 25, 0, 0.5, utc: true).iso(utc: true)
print Time.parse("2026-10-20T14:03:00Z") == t, Time.parse("2026-10-20t14:03z") == t
print Time.parse("2026-10-20T16:03:00+02:00") == t, Time.parse("2026-10-20T12:33:00-0130") == t
print Time.parse("2026-10-20 15:03:00+01").iso(utc: true)
print Time.parse("2026-10-20", utc: true).iso(utc: true), Time.parse("2026-10-20T14:03", utc: true) == t
print Time.parse("2026-10-20T14:03:00.25Z").iso(utc: true), Time.parse("2026-10-20T14:03:00,5Z").epoch
print Time.parse("2024-02-29T00:00:00Z").yday(utc: true)
print Time.parse(t.iso(utc: true)) == t, Time.parse(t.iso) == t
let bad = [
  "2026-10-2", "2026-13-01", "2026-02-29", "2026-10-20T24:00Z", "2026-10-20T14:60Z",
  "2026-10-20T14", "2026-10-20T14:03:00+25:00", "2026-10-20 junk", " 2026-10-20",
]
for s in bad
  try
    Time.parse s
  catch err: ConversionError
    print err.message
  end
end
let x: any = 1.5
try
  Time.new(2026, x)
catch err: TypeError
  print err.message
end
