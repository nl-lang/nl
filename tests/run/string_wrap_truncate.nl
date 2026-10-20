# wrap (greedy word wrap, with a measure function) and truncate (with omission and separator)
fn show(parts: [String])
  print "#{parts.len}: [#{Table.concat(parts, "|")}]"
end

show "the quick brown fox jumps over the lazy dog".wrap(10)
show "héllo wörld, ça va".wrap(11)
show "a  b \t c".wrap(80)

# A word wider than the line is broken between characters
show "abcdefghijkl xy".wrap(5)

# Each line is wrapped on its own; an empty line stays
show "one two\n\nthree".wrap(4)
show "".wrap(10)

# measure gives the width of a text: here two units per character
show "aa bb cc dd".wrap(10, measure: |t: String| -> t.length * 2)
# and here wide letters count double
let wide = |t: String| -> t.length + t.count("W") + t.count("M")
show "WM ab WW cd".wrap(4, measure: wide!)

# truncate: n characters at most, the omission included
let story = "Once upon a time in a world far far away"
print story.truncate(27)
print story.truncate(27, separator: " ")
print story.truncate(100)
print "héllo wörld".truncate(8, omission: "…")
print "héllo wörld".truncate(5, omission: "")
print "abc".truncate(2)
