# reverse, chars, codepoints, as_bytes, for ch in s, ord, chr, byteslice
let s = "héllo"
print s.reverse
print Table.concat(s.chars, ",")
print Table.concat(s.codepoints, ",")
print Table.concat(s.as_bytes, ",")
for ch in s
  print ch
end
print "é".ord
print "abc".ord
print 233.chr
print 104.chr
print s.byteslice(1, 3)
print s.byteslice(2, 3)
