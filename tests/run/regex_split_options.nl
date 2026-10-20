# split with a Regex separator takes keep_empty and limit, as with a string separator
let sep = Regex.new(',\s*')
fn show(parts: [String])
  print "#{parts.len}: [#{Table.concat(parts, "|")}]"
end

show "a, b,,c, ".split(sep)
show "a, b,,c, ".split(sep, keep_empty: true)
show "a, b,,c, ".split(sep, limit: 2)
show "".split(sep, keep_empty: true)
show "x,y".split(sep, limit: 1)
show sep.split("a,b,", keep_empty: true)
