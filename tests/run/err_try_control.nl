# return, break and continue inside try; try yields a value
fn early(x)
  try
    return "early #{x}" if x > 0
    print "no early return"
  catch
    print "unused"
  finally
    print "finally #{x}"
  end
  "late #{x}"
end

print early(1)
print early(0)

let seen = []
for i in 1..5
  try
    continue if i == 2
    break if i == 4
    Table.insert(seen, i)
  catch
    print "unused"
  end
end
print Table.concat(seen, ",")

fn value_from_try
  try
    "from body"
  catch
    "from catch"
  end
end

print value_from_try

fn value_from_catch
  try
    raise "x"
  catch
    "from catch"
  end
end

print value_from_catch
