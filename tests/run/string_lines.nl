# lines (without line endings), each_line, each_char, indent and dedent
fn show(parts: [String])
  print "#{parts.len}: [#{Table.concat(parts, "|")}]"
end

show "a\nb\r\n\nc".lines
show "one\ntwo\n".lines
show "".lines
show "\n".lines
print "x\ry".lines[1].len # a lone \r stays in the line

"héllo\nwörld".each_line do |line|
  print "line: #{line}"
end

let seen = []
"añb".each_char do |ch|
  Table.insert(seen, ch)
end
print Table.concat(seen, ",")

# indent: n copies of a padding before each line that is not empty
print "a\n\nb".indent(2)
print "a\nb".indent(1, "> ")
print "[" <> "x".indent(0) <> "]"

# dedent: the indentation every line shares; blank lines do not count
let text = "    déf main\n      print 1\n\n    end\n"
print text.dedent
print "  a\n\tb".dedent
print "[" <> "   \n  x".dedent <> "]"
