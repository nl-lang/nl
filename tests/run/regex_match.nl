# Match tables: [0], numbered and named groups, non-participating groups, init
let re = Regex.new '^(.+)uwu$'
let m = re.match "hewwo uwu"
print m[0]
print "[#{m[1]}]"
print re.match("nope") == nil
let date = Regex.new '(?<year>\d{4})-(?<month>\d{2})'
if let {year, month} = date.match("on 2026-10")
  print "#{month}/#{year}"
end
let d = date.match("2026-10")
print d[1]
print d[2]
let alt = Regex.new('(a)|(b)')
let am = alt.match("b")
print am[1] == nil
print am[2]
let later = Regex.new('\d').match("a1b2", 3)
print later[0]
