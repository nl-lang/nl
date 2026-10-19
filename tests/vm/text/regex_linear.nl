# Matching time is linear in the input; this pattern is catastrophic for a backtracking engine
let re = Regex.new('(x+x+)+y')
let s = :string.rep("x", 200000)
let t0 = :os.clock()
print re.test(s)
print re.match(s) == nil
print re.split(s).len
print re.replace_all(s, "-") == s
let elapsed = :os.clock() - t0
if elapsed < 5
  print "linear"
else
  print "too slow: #{elapsed}"
end
