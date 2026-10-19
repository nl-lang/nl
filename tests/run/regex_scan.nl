# scan iterates over the match tables of all matches
let re = Regex.new '\d+'
for m in re.scan("a1 b22 c333")
  print m[0]
end
let words = []
for m in "one two".scan(Regex.new('\w+'))
  Table.insert(words, m[0])
end
print Table.concat(words, "|")
