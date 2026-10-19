# `fn` followed by an LNAME, or a CNAME starting `Type.name`,
# declares a function; anything else is an anonymous function (here the last
# statement of its block).
fn foo
end
fn Foo.bar
end
fn make
  fn(x) x end
end
