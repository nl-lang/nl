# Time.humanize and ago_in_words, relative to a given time
let now = Time.at(1_792_504_980)
let before = now - 3.minutes
let after = now + 2.hours
print before.humanize(now), after.humanize(now)
print before.ago_in_words(now), after.ago_in_words(now)
for d in [0.5.seconds, 20.seconds, 40.days, 400.days]
  let past = now - d
  print past.humanize(now) <> " | " <> (now + d).humanize(now)
end
print now.humanize(now), now.ago_in_words(now)
print Time.now.humanize is String, (Time.now - 1.hours).humanize
let wrong: any = 5
try
  now.humanize(wrong)
catch err: TypeError
  print err.message
end
