# Unicode case mapping and character positions with multi-byte characters; a long iteration
print "straße".upcase
print "ÀÉÎ".downcase
print "émoji 😀!".length
print "émoji 😀!"[7]
print "😀x"[-1]
print "😀".ord
print Table.concat("a😀".codepoints, ",")
print Table.concat("é".as_bytes, ",")
print "héllo".reverse
print "héllo".index("l")
print "héllo".index("l", 4)
print "héllo"[2..3]
print "héllo"[2...3]
print "héllo"[-3..-2]
print "héllo"[9] == nil
print 128512.chr
print 233.chr
print "abc".center(7, "é")
let s = :string.rep("é", 1000) <> "x"
let n = 0
for ch in s
  n += 1
end
print n
print s.length
print s[1001]
print s[1000]
let total = 0
let i = 0
while i < 2000
  i += 1
  total += "ab€".length
end
print total
