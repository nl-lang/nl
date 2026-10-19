# A nil-safe member or method is a callee like any other; command arguments and a trailing block follow the ordinary rules.
a?.m x, y
o?.:m x
items?.each do |v|
  print v
end
:hud?.panel:show true
