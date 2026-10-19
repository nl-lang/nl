# Interfaces implemented for Number, String and Bool
interface Describe
  fn describe: String
end

interface Describe for Number
  fn describe: String
    "number #{self}"
  end
end

interface Describe for String
  fn describe: String
    "string of #{self.length} characters"
  end
end

interface Describe for Bool
  fn describe: String
    if self
      "yes"
    else
      "no"
    end
  end
end

print 5.describe
print "héllo".describe
print true.describe
print false.describe
let x = 7
print x.describe
