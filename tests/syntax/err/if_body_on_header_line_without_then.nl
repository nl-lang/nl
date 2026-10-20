# A body on the line of an `if` or `when` header needs `then` (or `;`):
# the error names `then`, and the next statement parses.
if ready go() end
case x
when 1 foo()
end
print "next"
