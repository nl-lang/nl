# split, and a Regex value in `when`
let parts = Regex.new(',\s*').split "a, b,c"
print Table.concat(parts, "|")
let greeting = Regex.new '^h'

fn classify(s)
  case s
  when greeting
    "starts with h"
  when 5
    "five"
  else
    "other"
  end
end

print classify("hello")
print classify("world")
print classify(5)
