# Many groups, named groups that did not participate, and the replacement syntax
let re = Regex.new(:string.rep("(a)", 40))
let m = re.match(:string.rep("a", 40))
print m[40]
print m[41] == nil
print m[0].len
let named = Regex.new('(?<a>x)(?<b>y)?')
let n = named.match("x")
print n.a
print n.b == nil
print n[2] == nil
print named.replace("x", '[$a][$b][$2][$zzz][$$][${a}]')
print named.replace_all("xyx", '<$b$a>')
