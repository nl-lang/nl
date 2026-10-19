# Regex.new, test, find (character positions), pattern, inline flags
let re = Regex.new '^(.+)uwu$'
print re.test("hewwo uwu")
print re.test("hello")
let first, last = re.find "hewwo uwu"
print "#{first} #{last}"
print re.pattern
let l = Regex.new('l+')
let f2, l2 = l.find("héllo")
print "#{f2} #{l2}"
let s = "héllo"
print s[f2..l2]
print l.find("abc") == nil
let o = Regex.new('o')
let f3, l3 = o.find("foo boo", 4)
print "#{f3} #{l3}"
print Regex.new('(?i)hello').test("HeLLo")
