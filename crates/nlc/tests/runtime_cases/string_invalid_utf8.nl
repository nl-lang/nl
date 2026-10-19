# Invalid UTF-8
# raises EncodingError instead of being dropped.
try
  print "a\xffb".length
catch e: EncodingError
  print typeof(e), e.message
end
