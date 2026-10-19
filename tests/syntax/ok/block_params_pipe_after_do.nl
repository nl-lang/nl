# `|` after `do` opens block parameters, `||` is an empty
# list, a top-level `|` closes the list; elsewhere `|` is bitwise or.
f do |a, b| a | b end
f do || 1 end
f do |a: (A | B), m = (x | y)| end
