# the count of a rest table is the number of arguments passed, or #args once items are appended
fn count(*args)
  :select("#", *args)
end

fn grow(*args)
  :table.insert(args, 9)
  print args.len, count(*args), [*args].len
end
grow(1, 2)
grow()

# A store into the table counts as well.
fn store(*args)
  args[3] = "c"
  print args.len, args[3]
end
store("a", "b")

# To shrink it, make a list of it first.
fn shrink(*args)
  let rest = [*args]
  rest.pop
  print args.len, rest.len
end
shrink(1, 2, 3)
