# scrub makes any bytes valid UTF-8: each invalid sequence becomes one replacement
fn bytes(s: String): String
  Table.concat(s.as_bytes, " ")
end

fn raw(*codes)
  :string.char(*codes)
end

let stray = raw(255) <> "abc"
print bytes(stray.scrub)
print stray.scrub("?")
print stray.scrub("?").valid_encoding

# A truncated character is one sequence; each stray byte is one
let cut = "ab" <> raw(0xE2, 0x82)
print cut.scrub("?")
print raw(0xE2, 0x82, 0xFF, 0x41).scrub("?")
print raw(0x80, 0x80, 0x41).scrub("?")

# Overlong forms and surrogates are invalid from their first byte
print raw(0xC0, 0xAF).scrub("?")
print raw(0xED, 0xA0, 0x80).scrub("?")

# Valid text comes back as it is, and the replacement may be any valid string
let between = "x" <> raw(0xFF) <> "y"
print "héllo wörld".scrub("?")
print between.scrub("«?»")
print between.scrub("")
let tail = "ok" <> raw(0xFF)
print tail.scrub.length

try
  print "x".scrub(raw(255))
catch err: EncodingError
  print "invalid replacement: EncodingError"
end

# Then the character functions work
let fixed = (raw(0xC3) <> "léa").scrub("_")
print fixed, fixed.length, fixed.reverse
