# Invalid UTF-8 raises EncodingError in character operations, never in byte-level ones
let bad = :string.char(104, 255)
print "héllo".valid_encoding
print bad.valid_encoding
print bad.len
print Table.concat(bad.as_bytes, ",")
print bad.byteslice(1, 1)
let joined = bad <> "!"
print joined.len
print "%d".format(bad.len)
print bad.rep(2).len
try
  let n = bad.length
catch err: EncodingError
  print "length: EncodingError"
end
try
  let u = bad.upcase
catch err: EncodingError
  print "upcase: EncodingError"
end
try
  for ch in bad
    print ch
  end
catch err: EncodingError
  print "iteration: EncodingError"
end
