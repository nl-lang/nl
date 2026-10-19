# Inside block parameters a top-level `|` closes the list;
# a union type or a bitwise-or default needs parentheses.
f do |a: (A | B), m = (x | y)| end
f do |m = x ^ y| end
