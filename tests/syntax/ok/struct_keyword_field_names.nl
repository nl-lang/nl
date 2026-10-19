# A keyword is a field name when `:` or `=` follows it on
# the same line, even a keyword that closes blocks (`end`, `when`, `else`).
struct Node
  type: String
  end = 0
  if: Bool = false
  when: Number
  else = nil
end
