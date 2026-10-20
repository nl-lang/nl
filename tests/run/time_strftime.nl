# Time.strftime in UTC: C's directives in the C locale, Ruby's %L and %P
let t = Time.at(1_792_504_980.25)
let fmt = fn(f) t.strftime(f, utc: true) end
print fmt("%Y-%m-%d %H:%M:%S.%L")
print fmt("%a %A %b %B %h")
print fmt("%y %C %j %e|%u %w")
print fmt("%I %l %p %P|%k")
print fmt("%F|%T|%D|%R|%r")
print fmt("%c|%x|%X")
print fmt("%z %Z %s")
print fmt("100%% %q")
let morning = Time.at(1_792_454_700)
print morning.strftime("%I:%M %p, %l %P, %H|%e", utc: true)
print Time.at(0).strftime("%c", utc: true)
