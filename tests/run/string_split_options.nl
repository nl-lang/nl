# split's keyed options: keep_empty keeps every field, limit caps the number of fields
fn show(parts: [String])
  print "#{parts.len}: [#{Table.concat(parts, "|")}]"
end

# Without options, Ruby's rules: trailing empty fields go, "" gives none
show "a,,b,".split(",")
show "".split(",")

# keep_empty: true keeps them all, and "" is one empty field
show "a,,b,".split(",", keep_empty: true)
show "".split(",", keep_empty: true)
show ",".split(",", keep_empty: true)

# limit: n gives at most n fields; the last one holds the rest
show "a,b,c,d".split(",", limit: 2)
show "a,b,,".split(",", limit: 3)
show "a,b,".split(",", limit: 5)
show "a,b".split(",", limit: 1)

# Without a separator: runs of whitespace, and the rest keeps its spaces
show " a b  c ".split
show " a b  c ".split(limit: 2)
show "a ".split(nil, limit: 2)

# The empty separator splits into characters
show "héllo".split("", limit: 3)

# Both options together, and a multi-byte separator
show "x--y----z".split("--", keep_empty: true, limit: 3)
show "wörld→and→more→".split("→", keep_empty: true)

try
  let n: any = 0
  "a,b".split(",", limit: n)
catch err: TypeError
  print "limit 0: TypeError"
end
try
  let opts: any = {bogus: true}
  "a,b".split(",", **opts)
catch err: TypeError
  print err.message
end
