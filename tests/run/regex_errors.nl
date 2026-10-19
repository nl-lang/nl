# RegexError for a bad pattern at runtime, EncodingError for an invalid subject
fn compile(p)
  Regex.new(p)
end

try
  compile("(")
catch err: RegexError
  print "RegexError"
end

let bad = :string.char(255)
try
  Regex.new('a').test(bad)
catch err: EncodingError
  print "EncodingError"
end
