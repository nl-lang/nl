# A string opener inside an interpolation starts a nested string.
a = "#{f("x")}"
b = "#{ "#{x}" }"
