# sub and gsub check what they insert, a
# replacement string or the string a replacement function returns, for
# valid UTF-8.
let bad = "\xff"
for f in [fn() "abc".gsub("b", bad) end, fn() "abc".sub("b", bad) end, fn() "abc".gsub("b", fn(m) bad end) end, fn() "abc".sub("b", fn(m) "x#{bad}" end) end]
  try
    print f().len
  catch e
    print "#{typeof(e)}: #{e.message}"
  end
end
print "abc".gsub("b", "é"), "abc".sub("b", fn(m) "[#{m[0]}]" end)

# Finding 12: a string indexed by a value that is neither a number nor a
# range raises TypeError, with the NL line in its trace.
try
  let x = "abc"[{}]
catch e
  print "#{typeof(e)}: #{e.message}"
  print e.trace.index("stack traceback:\n\tstring_replace_index.nl:17:") != nil
end
