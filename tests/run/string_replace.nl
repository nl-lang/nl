# sub and gsub with a string pattern, which is taken literally
let s = "héllo"
print s.sub("l", "L")
print s.gsub("l", "L")
print "a.b.c".gsub(".", "-")
print "a+b".sub("+", " plus ")
let shouted = "one two".gsub "o" do |m|
  m[0].upcase
end
print shouted
