# A line break inside `#{…}` of a one-line string; when the next
# line closes the interpolation and the string, the expression goes on there
# (one error).
a = "x #{ 1 +
2 }"
b = 1
