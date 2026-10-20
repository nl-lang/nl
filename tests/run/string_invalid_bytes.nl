# Byte functions work on strings that are not valid UTF-8; character functions still raise
let bad = :string.char(255) <> "abc,déf  "
print bad.valid_encoding, bad.len
print bad.start_with("x"), bad.end_with("  "), bad.include("déf"), bad.contains("z")
print bad.count("é"), bad.remove("abc").len, bad.is_empty, bad.is_blank
print bad.strip.len, bad.lstrip.len, bad.rstrip.len, bad.squish.len, (bad <> "\r\n").chomp.len
print "[" <> bad.delete_prefix(:string.char(255)) <> "]", bad.delete_suffix("  ").len
print bad.ensure_prefix(">").len, bad.ensure_suffix("!").len
print bad.split(",").len, "[" <> bad.split(",")[2] <> "]", bad.split.len, bad.lines.len
print bad.indent(2).len, (" " <> bad).dedent.len
print "#{bad.byteslice(2, 4)} #{"[{x}]".fmt(x: bad.byteslice(-5))}"

# An invalid needle is searched for as bytes too
let hi = :string.char(0xC3)
print "é".start_with(hi), "é".count(hi)

# What counts characters needs valid text
fn try_it(name: String, f: Function)
  try
    f()
    print "#{name}: no error"
  catch err: EncodingError
    print "#{name}: EncodingError"
  end
end
try_it "index", || -> bad.index("a")
try_it "length", || -> bad.length
try_it "chop", || -> bad.chop
try_it "split by chars", || -> bad.split("")
try_it "count of the empty string", || -> bad.count("")
try_it "snake_case", || -> bad.snake_case
try_it "split by comma", || -> bad.split(",")
