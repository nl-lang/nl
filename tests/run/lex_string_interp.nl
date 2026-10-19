# Double-quoted strings, escapes and interpolation (values go through tostring)
let name = "world"
let n = 3
print "hello, #{name}"
print "#{n} + 1 = #{n + 1}"
print "values: #{true} #{nil} #{1.5}"
print "literal \#{name}"
print "quote \" and backslash \\"
print "line1\nline2"
let t = {k: "v"}
print "member #{t.k}"
