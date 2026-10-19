# String methods that take a Regex: match, scan, sub, gsub, split
let digits = Regex.new '\d+'
let s = "a1b22"
print s.match(digits)[0]
let all = []
for m in s.scan(digits)
  Table.insert(all, m[0])
end
print Table.concat(all, ",")
print s.sub(digits, "#")
print s.gsub(digits, "#")
print Table.concat("a1b22c".split(digits), ",")
