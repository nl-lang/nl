# Errors and edge cases of the Regex methods, and String methods given a Regex
let five: any = 5
fn make(p)
  Regex.new(p)
end
try
  make('\p{Nope}')
catch err: RegexError
  print "RegexError"
end
try
  Regex.new(five)
catch err: TypeError
  print "TypeError"
end
try
  Regex.new('a').replace("a", five)
catch err: TypeError
  print "TypeError rep"
end
try
  Regex.new('a').test(five)
catch err: TypeError
  print "TypeError subject"
end
let r = Regex.new('\d')
print r.replace_all("a1b2", fn(m) tostring(tonumber(m[0]) * 2) end)
print r.find("abc") == nil
print r.find("a1", 10) == nil
print r.match("a1", 2)[0]
print r.match("a1", 3) == nil
let bad = :string.char(255)
try
  r.replace_all("1", fn(m) bad end)
catch err: EncodingError
  print "EncodingError result"
end
try
  "abc".sub(r, bad)
catch err: EncodingError
  print "EncodingError rep"
end
print "a1b22".gsub(r, "#")
print "a1b22".sub(r, fn(m) "<#{m[0]}>" end)
print Table.concat("a1b22c".split(r), "|")
print "a1b22".match(r, 3)[0]
