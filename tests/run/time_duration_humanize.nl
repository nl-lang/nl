# Duration.humanize: Rails-like buckets of distance_of_time_in_words
let samples = [
  0.5.seconds, 1.seconds, 44.seconds, 45.seconds, 89.seconds, 90.seconds,
  44.minutes, 45.minutes, 89.minutes, 90.minutes, 23.hours, 24.hours,
  41.hours, 42.hours, 29.days, 30.days, 45.days, 60.days, 200.days,
  364.days, 365.days, 500.days, 600.days, 700.days, 3650.days,
]
for d in samples
  print d.to_s, d.humanize
end
print((-3).minutes.humanize)
