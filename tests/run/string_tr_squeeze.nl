# tr (character translation, Ruby's sets) and squeeze (runs of one character)
print "hello".tr("el", "ip")
print "hello".tr("aeiou", "*")
print "hello".tr("aeiou", "AA*")
print "hello".tr("a-y", "b-z")
print "hello".tr("^aeiou", "*")
print "hello".tr("l", "")
print "héllo wörld".tr("éö", "eo")
print "héllo".tr("a-zé", "A-ZÉ")
print "a-b^c".tr("\\-\\^", "_~")
print "2024-10-20".tr("0-9", "#")

try
  let from: any = "z-a"
  print "x".tr(from, "y")
catch err
  print typeof(err)
end

print "aaabbbccc".squeeze
print "mississippi".squeeze
print "aaabbbccc".squeeze("a-b")
print "  héé  llo  ".squeeze(" ")
print "ööölla".squeeze("^l")
