# Comments and doc comments
# a whole-line comment
let a = 1 # a trailing comment
## A doc comment attaches to the next declaration.
fn documented
  "doc"
end
print a
print documented
print "# inside a string is not a comment"
print '# nor in a raw string'
