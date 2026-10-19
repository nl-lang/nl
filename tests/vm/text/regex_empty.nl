# Empty matches in scan, replace_all, split and find
print Regex.new('a*').replace_all("baab", "-")
print Table.concat(Regex.new('a*').split("baab"), "|")
print Table.concat(Regex.new('').split("abc"), "|")
print Regex.new('x*').replace_all("abc", "-")
print Regex.new('x*').replace("abc", "-")
let n = 0
for m in Regex.new('\b').scan("ab cd")
  n += 1
end
print n
let first, last = Regex.new('^').find("héllo")
print "#{first} #{last}"
let f2, l2 = Regex.new('$').find("héllo")
print "#{f2} #{l2}"
let f3, l3 = Regex.new('l').find("héllo", -2)
print "#{f3} #{l3}"
print Table.concat(Regex.new(',').split("a,b,,"), "|")
print Table.concat(Regex.new(',').split(",a,"), "|")
print Regex.new(',').split("").len
print Regex.new('').split("").len
