# to_number: Lua's tonumber, with an optional base; nil when the text is not a number
print "42".to_number + 1
print "3.5".to_number * 2
print "  7  ".to_number
print "1e3".to_number
print "0x1F".to_number
print "ff".to_number(16)
print "FF".to_number(16)
print "101".to_number(2)
print "z".to_number(36)
print "12abc".to_number == nil
print "".to_number == nil
print "héllo".to_number == nil
print "2".to_number(2) == nil

try
  let base: any = 1
  "1".to_number(base)
catch err: TypeError
  print "base 1: TypeError"
end
