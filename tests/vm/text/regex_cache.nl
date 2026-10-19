# Regex.new caches by pattern; a compiled pattern is a value of type Regex
print Regex.new('a+') == Regex.new('a+')
print Regex.new('a+') == Regex.new('a*')
print typeof(Regex.new('a'))
print Regex.new('a') is Regex
print "a" is Regex
print tostring(Regex.new('b'))
print Regex.new('[0-9]').pattern
fn pattern_of(re: Regex)
  re.pattern
end
print pattern_of(Regex.new('x|y'))
